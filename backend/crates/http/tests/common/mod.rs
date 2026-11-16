//! Shared builder and request helpers for API tests that need non-default
//! server settings. Requires `DATABASE_URL` (see README "Local services").
#![allow(dead_code)]

use std::{net::SocketAddr, sync::Arc};

use axum::{
    Router,
    body::Body,
    extract::ConnectInfo,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use http_body_util::BodyExt;
use rustchat_application::{
    auth::{AuthConfig, AuthService, RegistrationPolicy, SessionLifecycle},
    communities::{CommunityService, CommunityServiceConfig},
    conversations::{ConversationConfig, ConversationService},
    messages::{MessageConfig, MessageService},
    ports::RateLimitRule,
    ratelimit::{ApiRateLimitConfig, ApiRateLimiter, RateLimitConfig},
};
use rustchat_http::{AppEvents, AppState, ClientIpSource, HttpConfig, cluster, router};
use rustchat_infrastructure::{
    cluster::{PgEventBus, PgPresenceStore},
    mfa::AesGcmTotpEngine,
    postgres::{
        PgCommunityRepository, PgConversationRepository, PgMessageRepository, PgMfaRepository,
        PgOutboxRepository, PgSessionRepository, PgUserRepository,
    },
    ratelimit::InMemoryRateLimiter,
    security::{Argon2Hasher, OsTokenService, SystemClock},
};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;

pub const ORIGIN: &str = "https://chat.example.test";
pub const PASSWORD: &str = "correct horse battery";
pub const PEER: &str = "203.0.113.10:5000";

/// Records every email the server sends.
#[derive(Clone, Default)]
pub struct CapturedEmail(pub Arc<tokio::sync::Mutex<Vec<(String, String)>>>);

#[async_trait::async_trait]
impl rustchat_application::ports::EmailDelivery for CapturedEmail {
    async fn send(
        &self,
        recipient: &rustchat_domain::identity::EmailAddress,
        _subject: &str,
        text: &str,
    ) -> Result<(), rustchat_application::error::AppError> {
        self.0
            .lock()
            .await
            .push((recipient.as_str().to_owned(), text.to_owned()));
        Ok(())
    }
}

impl CapturedEmail {
    pub async fn count(&self) -> usize {
        self.0.lock().await.len()
    }

    /// The token in the newest link sent to `recipient`.
    pub async fn last_token_for(&self, recipient: &str) -> Option<String> {
        let sent = self.0.lock().await;
        let (_, body) = sent.iter().rev().find(|(to, _)| to == recipient)?;
        Some(
            body.split("token=")
                .nth(1)?
                .split_whitespace()
                .next()?
                .to_owned(),
        )
    }
}

pub struct Options {
    /// Run as one of several instances sharing the database: share realtime events through
    /// PostgreSQL, claim outbox events with leases and count presence across instances.
    /// The value identifies this instance.
    pub cluster: Option<uuid::Uuid>,
    /// Offer TOTP enrolment (real engine and repository, fixed test key).
    pub mfa: bool,
    /// Accounts treated as instance administrators.
    pub admin_user_ids: std::collections::HashSet<rustchat_domain::identity::UserId>,
    pub email: Option<CapturedEmail>,
    /// Enforce general API budgets with these limits when set.
    pub api_limits: Option<ApiRateLimitConfig>,
    pub lifecycle: SessionLifecycle,
    pub rate_limits: RateLimitConfig,
    pub client_ip: ClientIpSource,
}

impl Default for Options {
    fn default() -> Self {
        let generous = RateLimitRule {
            max: 10_000,
            window: time::Duration::hours(1),
        };
        Self {
            cluster: None,
            mfa: false,
            admin_user_ids: Default::default(),
            email: None,
            api_limits: None,
            lifecycle: SessionLifecycle::default(),
            rate_limits: RateLimitConfig {
                login_per_ip: generous,
                login_failures_per_user: generous,
                register_per_ip: generous,
            },
            client_ip: ClientIpSource::Peer,
        }
    }
}

/// A running server instance's router plus the handle that makes its sockets resynchronize.
pub struct Node {
    pub router: Router,
    pub resync: tokio::sync::broadcast::Sender<()>,
}

pub fn app(pool: PgPool, options: Options) -> Router {
    node(pool, options).router
}

pub fn node(pool: PgPool, options: Options) -> Node {
    let session_ttl = time::Duration::days(30);
    let auth = AuthService::new(
        Arc::new(PgUserRepository::new(pool.clone())),
        Arc::new(PgSessionRepository::new(pool.clone())),
        // Low Argon2 costs keep the suite fast; the algorithm is unchanged.
        Arc::new(Argon2Hasher::with_costs(8, 1, 1).unwrap()),
        Arc::new(OsTokenService),
        Arc::new(SystemClock),
        Arc::new(InMemoryRateLimiter::new()),
        AuthConfig {
            registration: RegistrationPolicy::Open,
            session_ttl,
            rate_limits: options.rate_limits,
        },
    )
    .with_session_lifecycle(options.lifecycle)
    .with_instance_admins(options.admin_user_ids);
    let auth = if options.mfa {
        auth.with_mfa(
            Arc::new(PgMfaRepository::new(pool.clone())),
            Arc::new(AesGcmTotpEngine::new([9u8; 32], "RustChat")),
        )
    } else {
        auth
    };
    let auth = match options.email {
        Some(email) => auth.with_email_delivery(Arc::new(email), ORIGIN.to_owned()),
        None => auth,
    };
    let conversations = ConversationService::new(
        Arc::new(PgUserRepository::new(pool.clone())),
        Arc::new(PgConversationRepository::new(pool.clone())),
        ConversationConfig {
            max_members: 100,
            max_page_size: 100,
        },
    );
    let messages = MessageService::new(
        Arc::new(PgMessageRepository::new(pool.clone())),
        MessageConfig { max_page_size: 100 },
    );
    let communities = CommunityService::new(
        Arc::new(PgCommunityRepository::new(pool.clone())),
        Arc::new(OsTokenService),
        Arc::new(SystemClock),
        CommunityServiceConfig {
            max_invite_ttl: time::Duration::days(30),
        },
    );
    let events = AppEvents {
        messages: tokio::sync::broadcast::channel(256).0,
        typing: tokio::sync::broadcast::channel(256).0,
        read_cursors: tokio::sync::broadcast::channel(256).0,
        reactions: tokio::sync::broadcast::channel(256).0,
        delivery_cursors: tokio::sync::broadcast::channel(256).0,
        presence: tokio::sync::broadcast::channel(256).0,
        community: tokio::sync::broadcast::channel(256).0,
    };
    let (resync, _) = tokio::sync::broadcast::channel(16);
    let mut state = AppState::new(
        Arc::new(auth),
        Arc::new(conversations),
        Arc::new(messages),
        Arc::new(communities),
        events.clone(),
        HttpConfig {
            allowed_origins: vec![ORIGIN.to_owned()],
            cookie_secure: true,
            session_ttl,
            client_ip: options.client_ip,
        },
    );
    if let Some(limits) = options.api_limits {
        state = state.with_api_rate_limiter(Arc::new(ApiRateLimiter::new(
            Arc::new(InMemoryRateLimiter::new()),
            Arc::new(SystemClock),
            limits,
        )));
    }
    if let Some(instance) = options.cluster {
        let outbox = Arc::new(PgOutboxRepository::new(pool.clone()));
        let bus = PgEventBus::start(
            pool.clone(),
            instance,
            cluster::remote_event_handler(events.clone(), outbox.clone()),
            cluster::gap_handler(resync.clone()),
        );
        state = state.with_cluster(
            bus.clone(),
            Arc::new(PgPresenceStore::new(pool, instance)),
            resync.clone(),
        );
        // Detached: it runs for the life of the test's runtime.
        cluster::spawn_outbox_dispatcher(outbox, events.messages.clone(), bus);
    }
    Node {
        router: router(state),
        resync,
    }
}

pub struct Reply {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub json: Value,
}

impl Reply {
    pub fn code(&self) -> &str {
        self.json["error"]["code"].as_str().unwrap_or("<none>")
    }

    /// Every `Set-Cookie` value for the session cookie, as `name=value` pairs.
    pub fn session_cookies(&self) -> Vec<String> {
        self.headers
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .filter(|value| value.starts_with("__Host-rustchat_session="))
            .map(|value| value.split(';').next().unwrap().to_owned())
            .collect()
    }

    /// The single non-empty session cookie set by this reply, if any.
    pub fn new_session_cookie(&self) -> Option<String> {
        self.session_cookies()
            .into_iter()
            .find(|cookie| !cookie.ends_with('='))
    }
}

pub async fn call(
    app: &Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
    cookie: Option<&str>,
) -> Reply {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    builder = builder.header(header::ORIGIN, ORIGIN);
    let request_body = match body {
        Some(value) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };
    let mut request = builder.body(request_body).unwrap();
    request
        .extensions_mut()
        .insert(ConnectInfo(PEER.parse::<SocketAddr>().unwrap()));
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    Reply {
        status,
        headers,
        json: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    }
}

pub async fn register(app: &Router, username: &str) -> Reply {
    call(
        app,
        Method::POST,
        "/api/v1/auth/register",
        Some(json!({"username": username, "password": PASSWORD})),
        None,
    )
    .await
}

/// Registers and logs in; returns the session cookie pair.
pub async fn sign_in(app: &Router, username: &str) -> String {
    assert_eq!(register(app, username).await.status, StatusCode::CREATED);
    let reply = call(
        app,
        Method::POST,
        "/api/v1/auth/login",
        Some(json!({"username": username, "password": PASSWORD})),
        None,
    )
    .await;
    assert_eq!(
        reply.status,
        StatusCode::OK,
        "login failed: {:?}",
        reply.json
    );
    reply.new_session_cookie().expect("session cookie")
}

/// Logs in an already registered account; returns the session cookie pair.
pub async fn sign_in_existing(app: &Router, username: &str) -> String {
    let reply = call(
        app,
        Method::POST,
        "/api/v1/auth/login",
        Some(json!({"username": username, "password": PASSWORD})),
        None,
    )
    .await;
    assert_eq!(
        reply.status,
        StatusCode::OK,
        "login failed: {:?}",
        reply.json
    );
    reply.new_session_cookie().expect("session cookie")
}
