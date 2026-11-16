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
    ratelimit::RateLimitConfig,
};
use rustchat_http::{AppEvents, AppState, ClientIpSource, HttpConfig, router};
use rustchat_infrastructure::{
    postgres::{
        PgCommunityRepository, PgConversationRepository, PgMessageRepository, PgSessionRepository,
        PgUserRepository,
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

pub struct Options {
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

pub fn app(pool: PgPool, options: Options) -> Router {
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
    .with_session_lifecycle(options.lifecycle);
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
        Arc::new(PgCommunityRepository::new(pool)),
        Arc::new(OsTokenService),
        Arc::new(SystemClock),
        CommunityServiceConfig {
            max_invite_ttl: time::Duration::days(30),
        },
    );
    router(AppState::new(
        Arc::new(auth),
        Arc::new(conversations),
        Arc::new(messages),
        Arc::new(communities),
        AppEvents {
            messages: tokio::sync::broadcast::channel(256).0,
            typing: tokio::sync::broadcast::channel(256).0,
            read_cursors: tokio::sync::broadcast::channel(256).0,
            reactions: tokio::sync::broadcast::channel(256).0,
            delivery_cursors: tokio::sync::broadcast::channel(256).0,
            presence: tokio::sync::broadcast::channel(256).0,
            community: tokio::sync::broadcast::channel(256).0,
        },
        HttpConfig {
            allowed_origins: vec![ORIGIN.to_owned()],
            cookie_secure: true,
            session_ttl,
            client_ip: options.client_ip,
        },
    ))
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
