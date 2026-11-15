//! End-to-end API tests: real router, real PostgreSQL, real Argon2id.
//! Requires `DATABASE_URL` (see README "Local services").

use std::{collections::HashSet, net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    Router,
    body::Body,
    extract::ConnectInfo,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use futures_util::{SinkExt, StreamExt};
use http_body_util::BodyExt;
use rustchat_application::{
    auth::{AuthConfig, AuthService, RegistrationPolicy},
    communities::{CommunityService, CommunityServiceConfig},
    conversations::{ConversationConfig, ConversationService},
    error::AppError,
    messages::{MessageConfig, MessageService},
    ports::{EmailDelivery, RateLimitRule, TokenService},
    ratelimit::RateLimitConfig,
};
use rustchat_domain::{
    identity::{ConversationId, MessageId, UserId},
    message::{Message, MessageContent},
};
use rustchat_http::{
    AppEvents, AppState, ClientIpSource, CommunityEvent, DeliveryCursorEvent, HttpConfig,
    ReactionEvent, ReadCursorEvent, router,
};
use rustchat_infrastructure::{
    MIGRATOR,
    postgres::{
        PgCommunityRepository, PgConversationRepository, PgMessageRepository, PgSessionRepository,
        PgUserRepository,
    },
    ratelimit::InMemoryRateLimiter,
    security::{Argon2Hasher, OsTokenService, SystemClock},
};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message as TungsteniteMessage, client::IntoClientRequest},
};
use tower::ServiceExt;
use uuid::Uuid;

const ORIGIN: &str = "https://chat.example.test";
const PASSWORD: &str = "correct horse battery";
const COOKIE_NAME: &str = "__Host-rustchat_session";
const PEER: &str = "203.0.113.50:40000";

/// Limits high enough that these tests, which are not about rate limiting,
/// never reach them. Rate limiting is covered in `rate_limit_api.rs`.
fn generous_limits() -> RateLimitConfig {
    let rule = RateLimitRule {
        max: 10_000,
        window: time::Duration::hours(1),
    };
    RateLimitConfig {
        login_per_ip: rule,
        login_failures_per_user: rule,
        register_per_ip: rule,
    }
}

fn app_with(pool: PgPool, registration: RegistrationPolicy) -> Router {
    app_with_member_limit(pool, registration, 100)
}

fn app_with_member_limit(
    pool: PgPool,
    registration: RegistrationPolicy,
    max_members: usize,
) -> Router {
    app_with_member_limit_and_email(pool, registration, max_members, None)
}

fn app_with_email(pool: PgPool, delivery: Arc<dyn EmailDelivery>) -> Router {
    app_with_member_limit_and_email(pool, RegistrationPolicy::Open, 100, Some(delivery))
}

fn app_with_member_limit_and_email(
    pool: PgPool,
    registration: RegistrationPolicy,
    max_members: usize,
    email_delivery: Option<Arc<dyn EmailDelivery>>,
) -> Router {
    let ready_pool = pool.clone();
    app_with_readiness_probe(
        pool,
        registration,
        max_members,
        email_delivery,
        None,
        HashSet::new(),
        move || {
            let pool = ready_pool.clone();
            async move { rustchat_infrastructure::postgres::ready(&pool).await }
        },
    )
}

fn app_with_readiness_probe<F, Fut>(
    pool: PgPool,
    registration: RegistrationPolicy,
    max_members: usize,
    email_delivery: Option<Arc<dyn EmailDelivery>>,
    events: Option<AppEvents>,
    admin_user_ids: HashSet<UserId>,
    readiness_probe: F,
) -> Router
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = bool> + Send + 'static,
{
    let session_ttl = time::Duration::days(30);
    let mut auth = AuthService::new(
        Arc::new(PgUserRepository::new(pool.clone())),
        Arc::new(PgSessionRepository::new(pool.clone())),
        // Low Argon2 costs keep the suite fast; the algorithm is unchanged.
        Arc::new(Argon2Hasher::with_costs(8, 1, 1).unwrap()),
        Arc::new(OsTokenService),
        Arc::new(SystemClock),
        Arc::new(InMemoryRateLimiter::new()),
        AuthConfig {
            registration,
            session_ttl,
            rate_limits: generous_limits(),
        },
    )
    .with_instance_admins(admin_user_ids);
    if let Some(delivery) = email_delivery {
        auth = auth.with_email_delivery(delivery, ORIGIN.to_owned());
    }
    let conversations = ConversationService::new(
        Arc::new(PgUserRepository::new(pool.clone())),
        Arc::new(PgConversationRepository::new(pool.clone())),
        ConversationConfig {
            max_members,
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
    let events = events.unwrap_or_else(|| AppEvents {
        messages: tokio::sync::broadcast::channel(256).0,
        typing: tokio::sync::broadcast::channel(256).0,
        read_cursors: tokio::sync::broadcast::channel(256).0,
        reactions: tokio::sync::broadcast::channel(256).0,
        delivery_cursors: tokio::sync::broadcast::channel(256).0,
        presence: tokio::sync::broadcast::channel(256).0,
        community: tokio::sync::broadcast::channel(256).0,
    });
    router(
        AppState::new(
            Arc::new(auth),
            Arc::new(conversations),
            Arc::new(messages),
            Arc::new(communities),
            events,
            HttpConfig {
                allowed_origins: vec![ORIGIN.to_owned()],
                cookie_secure: true,
                session_ttl,
                client_ip: ClientIpSource::Peer,
            },
        )
        .with_readiness_probe(readiness_probe),
    )
}

fn app(pool: PgPool) -> Router {
    app_with(pool, RegistrationPolicy::Open)
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    json: Value,
}

#[derive(Clone, Default)]
struct CapturedEmail(Arc<tokio::sync::Mutex<Vec<String>>>);

#[async_trait::async_trait]
impl EmailDelivery for CapturedEmail {
    async fn send(
        &self,
        _recipient: &rustchat_domain::identity::EmailAddress,
        _subject: &str,
        text: &str,
    ) -> Result<(), AppError> {
        self.0.lock().await.push(text.to_owned());
        Ok(())
    }
}

async fn latest_email_token(delivery: &CapturedEmail) -> String {
    let messages = delivery.0.lock().await;
    let body = messages.last().expect("email delivery captured");
    body.split("token=")
        .nth(1)
        .and_then(|value| value.split_whitespace().next())
        .expect("token in email link")
        .to_owned()
}

impl Reply {
    fn error_code(&self) -> &str {
        self.json["error"]["code"].as_str().unwrap_or("<none>")
    }

    /// The `name=value` pair of the session cookie, if one was set.
    fn session_cookie(&self) -> Option<String> {
        self.headers
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .find(|v| v.starts_with(COOKIE_NAME))
            .and_then(|v| v.split(';').next())
            .map(str::to_owned)
    }

    fn set_cookie_header(&self) -> String {
        self.headers
            .get(header::SET_COOKIE)
            .expect("Set-Cookie header")
            .to_str()
            .unwrap()
            .to_owned()
    }
}

struct Call<'a> {
    method: Method,
    uri: &'a str,
    body: Option<Value>,
    cookie: Option<&'a str>,
    origin: Option<&'a str>,
}

fn add_reaction_call<'a>(uri: &'a str, cookie: &'a str) -> Call<'a> {
    Call {
        method: Method::PUT,
        uri,
        body: Some(json!({"emoji":"👍"})),
        cookie: Some(cookie),
        origin: Some(ORIGIN),
    }
}

async fn send(app: &Router, call: Call<'_>) -> Reply {
    let mut builder = Request::builder().method(call.method).uri(call.uri);
    if let Some(cookie) = call.cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    if let Some(origin) = call.origin {
        builder = builder.header(header::ORIGIN, origin);
    }
    let body = match call.body {
        Some(value) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };
    let mut request = builder.body(body).unwrap();
    request
        .extensions_mut()
        .insert(ConnectInfo(PEER.parse::<SocketAddr>().unwrap()));
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    Reply {
        status,
        headers,
        json,
    }
}

async fn connect_websocket(
    url: &str,
    cookie: &str,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let mut request = url.into_client_request().unwrap();
    request
        .headers_mut()
        .insert(header::COOKIE, cookie.parse().unwrap());
    request
        .headers_mut()
        .insert(header::ORIGIN, ORIGIN.parse().unwrap());
    connect_async(request).await.unwrap().0
}

async fn next_websocket_json<S>(socket: &mut S) -> Value
where
    S: futures_util::Stream<
            Item = Result<TungsteniteMessage, tokio_tungstenite::tungstenite::Error>,
        > + Unpin,
{
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(Ok(TungsteniteMessage::Text(text))) = socket.next().await {
                return serde_json::from_str(text.as_str()).unwrap();
            }
        }
    })
    .await
    .expect("websocket event timed out")
}

async fn post_json(app: &Router, uri: &str, body: Value) -> Reply {
    send(
        app,
        Call {
            method: Method::POST,
            uri,
            body: Some(body),
            cookie: None,
            origin: Some(ORIGIN),
        },
    )
    .await
}

async fn get(app: &Router, uri: &str, cookie: Option<&str>) -> Reply {
    send(
        app,
        Call {
            method: Method::GET,
            uri,
            body: None,
            cookie,
            origin: None,
        },
    )
    .await
}

async fn authed(app: &Router, method: Method, uri: &str, cookie: &str) -> Reply {
    send(
        app,
        Call {
            method,
            uri,
            body: None,
            cookie: Some(cookie),
            origin: Some(ORIGIN),
        },
    )
    .await
}

async fn register(app: &Router, username: &str) -> Reply {
    post_json(
        app,
        "/api/v1/auth/register",
        json!({"username": username, "password": PASSWORD}),
    )
    .await
}

/// Register (if needed) and log in; returns the session cookie pair.
async fn login_cookie(app: &Router, username: &str) -> String {
    let reply = post_json(
        app,
        "/api/v1/auth/login",
        json!({"username": username, "password": PASSWORD}),
    )
    .await;
    assert_eq!(
        reply.status,
        StatusCode::OK,
        "login failed: {:?}",
        reply.json
    );
    reply.session_cookie().expect("session cookie")
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn healthz_is_ok(pool: PgPool) {
    let application = app(pool.clone());
    let reply = get(&application, "/healthz", None).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.json, json!({"status": "ok"}));
    let ready = get(&application, "/readyz", None).await;
    assert_eq!(ready.status, StatusCode::OK);
    assert_eq!(ready.json, json!({"status": "ok"}));
    let metrics_response = application
        .clone()
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(metrics_response.status(), StatusCode::OK);
    assert_eq!(
        metrics_response.headers()[header::CONTENT_TYPE],
        "text/plain; version=0.0.4; charset=utf-8"
    );
    let metrics_body = metrics_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let metrics_text = String::from_utf8(metrics_body.to_vec()).unwrap();
    assert!(metrics_text.contains("rustchat_http_responses_total{status_class=\"2xx\"} 2"));
    let unavailable = app_with_readiness_probe(
        pool,
        RegistrationPolicy::Open,
        100,
        None,
        None,
        HashSet::new(),
        || async { false },
    );
    let not_ready = get(&unavailable, "/readyz", None).await;
    assert_eq!(not_ready.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(not_ready.json, json!({"status": "unavailable"}));
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn register_creates_a_user_without_leaking_secrets(pool: PgPool) {
    let app = app(pool);
    let reply = register(&app, "Alice").await;
    assert_eq!(reply.status, StatusCode::CREATED);
    assert_eq!(reply.json["user"]["username"], "alice");
    assert!(reply.json["user"]["id"].is_string());
    let text = reply.json.to_string();
    assert!(!text.contains("password"), "response leaked: {text}");
    assert!(reply.session_cookie().is_none(), "register must not log in");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn instance_admins_can_list_and_suspend_users_with_audited_session_revocation(pool: PgPool) {
    let bootstrap = app(pool.clone());
    let alice_registration = register(&bootstrap, "alice").await;
    let bobby_registration = register(&bootstrap, "bobby").await;
    let alice_id =
        Uuid::parse_str(alice_registration.json["user"]["id"].as_str().unwrap()).unwrap();
    let bobby_id =
        Uuid::parse_str(bobby_registration.json["user"]["id"].as_str().unwrap()).unwrap();
    let alice = login_cookie(&bootstrap, "alice").await;
    let bobby = login_cookie(&bootstrap, "bobby").await;

    let admin_app = app_with_readiness_probe(
        pool.clone(),
        RegistrationPolicy::Open,
        100,
        None,
        None,
        HashSet::from([UserId::from_uuid(alice_id)]),
        || async { true },
    );
    let users = get(&admin_app, "/api/v1/admin/users", Some(&alice)).await;
    assert_eq!(users.status, StatusCode::OK);
    assert_eq!(users.json["users"].as_array().unwrap().len(), 2);
    assert!(users.json["users"][0].get("password_hash").is_none());
    assert_eq!(
        get(&admin_app, "/api/v1/admin/users", Some(&bobby))
            .await
            .status,
        StatusCode::FORBIDDEN
    );

    let suspension_uri = format!("/api/v1/admin/users/{bobby_id}/suspension");
    let self_suspension = send(
        &admin_app,
        Call {
            method: Method::PUT,
            uri: &format!("/api/v1/admin/users/{alice_id}/suspension"),
            body: Some(json!({"suspended":true})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(self_suspension.status, StatusCode::CONFLICT);
    assert_eq!(self_suspension.error_code(), "CANNOT_SUSPEND_SELF");

    let suspended = send(
        &admin_app,
        Call {
            method: Method::PUT,
            uri: &suspension_uri,
            body: Some(json!({"suspended":true})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(suspended.status, StatusCode::OK);
    assert!(!suspended.json["user"]["suspended_at"].is_null());
    assert_eq!(
        get(&admin_app, "/api/v1/auth/me", Some(&bobby))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(
            &admin_app,
            Call {
                method: Method::POST,
                uri: "/api/v1/auth/login",
                body: Some(json!({"username":"bobby", "password":PASSWORD})),
                cookie: None,
                origin: Some(ORIGIN),
            }
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );

    let unsuspended = send(
        &admin_app,
        Call {
            method: Method::PUT,
            uri: &suspension_uri,
            body: Some(json!({"suspended":false})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(unsuspended.status, StatusCode::OK);
    assert!(unsuspended.json["user"]["suspended_at"].is_null());
    assert_eq!(
        send(
            &admin_app,
            Call {
                method: Method::POST,
                uri: "/api/v1/auth/login",
                body: Some(json!({"username":"bobby", "password":PASSWORD})),
                cookie: None,
                origin: Some(ORIGIN),
            }
        )
        .await
        .status,
        StatusCode::OK
    );

    let audit_count: i64 = sqlx::query_scalar("SELECT count(*) FROM instance_admin_audit_log WHERE actor_user_id = $1 AND target_user_id = $2")
        .bind(alice_id)
        .bind(bobby_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(audit_count, 2);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn password_reset_is_single_use_and_revokes_existing_sessions(pool: PgPool) {
    let delivery = CapturedEmail::default();
    let pool_for_expiry = pool.clone();
    let app = app_with_email(pool, Arc::new(delivery.clone()));
    let registered = post_json(
        &app,
        "/api/v1/auth/register",
        json!({"username":"recoverable","email":"Recoverable@example.test","password":PASSWORD}),
    )
    .await;
    assert_eq!(registered.status, StatusCode::CREATED);
    let session = login_cookie(&app, "recoverable").await;

    let before_unknown_request = delivery.0.lock().await.len();
    let unknown_email = post_json(
        &app,
        "/api/v1/auth/password-reset/request",
        json!({"email":"unknown@example.test"}),
    )
    .await;
    assert_eq!(unknown_email.status, StatusCode::ACCEPTED);
    assert_eq!(delivery.0.lock().await.len(), before_unknown_request);
    let reset_requested = post_json(
        &app,
        "/api/v1/auth/password-reset/request",
        json!({"email":"recoverable@example.test"}),
    )
    .await;
    assert_eq!(reset_requested.status, StatusCode::ACCEPTED);
    let reset_token = latest_email_token(&delivery).await;

    let reset = post_json(
        &app,
        "/api/v1/auth/password-reset/complete",
        json!({"token":reset_token,"new_password":"a different secure password"}),
    )
    .await;
    assert_eq!(reset.status, StatusCode::NO_CONTENT);
    let reused = post_json(
        &app,
        "/api/v1/auth/password-reset/complete",
        json!({"token":reset_token,"new_password":"another secure password"}),
    )
    .await;
    assert_eq!(reused.status, StatusCode::BAD_REQUEST);
    let old_session = get(&app, "/api/v1/auth/profile", Some(&session)).await;
    assert_eq!(old_session.status, StatusCode::UNAUTHORIZED);
    let new_login = post_json(
        &app,
        "/api/v1/auth/login",
        json!({"username":"recoverable","password":"a different secure password"}),
    )
    .await;
    assert_eq!(new_login.status, StatusCode::OK);

    let _ = post_json(
        &app,
        "/api/v1/auth/password-reset/request",
        json!({"email":"recoverable@example.test"}),
    )
    .await;
    let expired_token = latest_email_token(&delivery).await;
    let digest = OsTokenService.digest(&expired_token);
    sqlx::query("UPDATE account_tokens SET expires_at = created_at + interval '1 microsecond' WHERE token_digest = $1")
        .bind(digest.0.to_vec())
        .execute(&pool_for_expiry)
        .await
        .unwrap();
    let expired = post_json(
        &app,
        "/api/v1/auth/password-reset/complete",
        json!({"token":expired_token,"new_password":"another secure password"}),
    )
    .await;
    assert_eq!(expired.status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn profiles_are_private_authenticated_and_validated(pool: PgPool) {
    let app = app(pool);
    register(&app, "alice").await;
    let alice = login_cookie(&app, "alice").await;

    let unauthorized = get(&app, "/api/v1/auth/profile", None).await;
    assert_eq!(unauthorized.status, StatusCode::UNAUTHORIZED);

    let initial = get(&app, "/api/v1/auth/profile", Some(&alice)).await;
    assert_eq!(initial.status, StatusCode::OK);
    assert_eq!(initial.json["profile"]["username"], "alice");
    assert_eq!(initial.json["profile"]["display_name"], "alice");

    let updated = send(
        &app,
        Call {
            method: Method::PATCH,
            uri: "/api/v1/auth/profile",
            body: Some(json!({"display_name":"Alice R.", "bio":"Here to chat."})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(updated.status, StatusCode::OK);
    assert_eq!(updated.json["profile"]["display_name"], "Alice R.");
    assert_eq!(updated.json["profile"]["bio"], "Here to chat.");

    let invalid = send(
        &app,
        Call {
            method: Method::PATCH,
            uri: "/api/v1/auth/profile",
            body: Some(json!({"display_name":"x".repeat(65), "bio":""})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(invalid.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(invalid.error_code(), "VALIDATION_ERROR");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn username_can_be_changed_with_validation_and_uniqueness(pool: PgPool) {
    let app = app(pool);
    register(&app, "alice").await;
    register(&app, "bobby").await;
    let alice = login_cookie(&app, "alice").await;
    let update = |username: &str| Call {
        method: Method::PATCH,
        uri: "/api/v1/auth/username",
        body: Some(json!({"username": username})),
        cookie: Some(&alice),
        origin: Some(ORIGIN),
    };
    let changed = send(&app, update("alice_new")).await;
    assert_eq!(changed.status, StatusCode::OK);
    assert_eq!(changed.json["user"]["username"], "alice_new");

    let duplicate = send(&app, update("bobby")).await;
    assert_eq!(duplicate.status, StatusCode::CONFLICT);
    assert_eq!(duplicate.error_code(), "USERNAME_TAKEN");

    let invalid = send(&app, update("x!")).await;
    assert_eq!(invalid.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn profile_privacy_controls_discovery_and_shared_conversation_visibility(pool: PgPool) {
    let app = app(pool);
    for username in ["alice", "bobby", "carol"] {
        register(&app, username).await;
    }
    let alice = login_cookie(&app, "alice").await;
    let bobby = login_cookie(&app, "bobby").await;
    let carol = login_cookie(&app, "carol").await;

    let direct = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/conversations",
            body: Some(json!({"type":"direct","username":"alice"})),
            cookie: Some(&bobby),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(direct.status, StatusCode::CREATED, "{:?}", direct.json);
    let public_profile = get(&app, "/api/v1/users/alice/profile", Some(&carol)).await;
    assert_eq!(public_profile.status, StatusCode::OK);
    assert_eq!(public_profile.json["profile"]["username"], "alice");

    let updated_privacy = send(
        &app,
        Call {
            method: Method::PATCH,
            uri: "/api/v1/auth/privacy",
            body: Some(json!({"discoverable":false,"profile_visibility":"conversation_members"})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(
        updated_privacy.status,
        StatusCode::OK,
        "{:?}",
        updated_privacy.json
    );
    assert_eq!(updated_privacy.json["privacy"]["discoverable"], false);

    let hidden_profile = get(&app, "/api/v1/users/alice/profile", Some(&carol)).await;
    assert_eq!(hidden_profile.status, StatusCode::NOT_FOUND);
    let shared_profile = get(&app, "/api/v1/users/alice/profile", Some(&bobby)).await;
    assert_eq!(shared_profile.status, StatusCode::OK);

    let hidden_direct = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/conversations",
            body: Some(json!({"type":"direct","username":"alice"})),
            cookie: Some(&carol),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(hidden_direct.status, StatusCode::NOT_FOUND);
    let own_privacy = get(&app, "/api/v1/auth/privacy", Some(&alice)).await;
    assert_eq!(
        own_privacy.json["privacy"]["profile_visibility"],
        "conversation_members"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn register_rejects_a_taken_username_case_insensitively(pool: PgPool) {
    let app = app(pool);
    assert_eq!(register(&app, "alice").await.status, StatusCode::CREATED);
    let again = register(&app, "ALICE").await;
    assert_eq!(again.status, StatusCode::CONFLICT);
    assert_eq!(again.error_code(), "USERNAME_TAKEN");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn register_validates_input(pool: PgPool) {
    let app = app(pool);
    let bad_name = register(&app, "no spaces allowed").await;
    assert_eq!(bad_name.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(bad_name.error_code(), "VALIDATION_ERROR");

    let weak = post_json(
        &app,
        "/api/v1/auth/register",
        json!({"username": "alice", "password": "short"}),
    )
    .await;
    assert_eq!(weak.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(weak.error_code(), "VALIDATION_ERROR");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn register_is_refused_when_closed(pool: PgPool) {
    let app = app_with(pool, RegistrationPolicy::Closed);
    let reply = register(&app, "alice").await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN);
    assert_eq!(reply.error_code(), "REGISTRATION_CLOSED");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn login_sets_a_hardened_session_cookie(pool: PgPool) {
    let app = app(pool);
    register(&app, "alice").await;
    let reply = post_json(
        &app,
        "/api/v1/auth/login",
        json!({"username": "Alice", "password": PASSWORD}),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.json["user"]["username"], "alice");

    let cookie = reply.set_cookie_header();
    assert!(cookie.starts_with("__Host-rustchat_session="), "{cookie}");
    for attribute in [
        "HttpOnly",
        "Secure",
        "SameSite=Strict",
        "Path=/",
        "Max-Age=",
    ] {
        assert!(cookie.contains(attribute), "missing {attribute}: {cookie}");
    }
    assert!(
        !cookie.contains("Domain"),
        "__Host- cookies must not set Domain"
    );
    assert_eq!(reply.headers[header::CACHE_CONTROL], "no-store");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn failed_logins_are_indistinguishable(pool: PgPool) {
    let app = app(pool);
    register(&app, "alice").await;
    let wrong_password = post_json(
        &app,
        "/api/v1/auth/login",
        json!({"username": "alice", "password": "wrong password!!"}),
    )
    .await;
    let unknown_user = post_json(
        &app,
        "/api/v1/auth/login",
        json!({"username": "nobody", "password": "wrong password!!"}),
    )
    .await;
    let malformed_name = post_json(
        &app,
        "/api/v1/auth/login",
        json!({"username": "no spaces!", "password": "wrong password!!"}),
    )
    .await;
    for reply in [&wrong_password, &unknown_user, &malformed_name] {
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
        assert_eq!(reply.error_code(), "INVALID_CREDENTIALS");
        assert!(reply.session_cookie().is_none());
    }
    assert_eq!(wrong_password.json, unknown_user.json);
    assert_eq!(wrong_password.json, malformed_name.json);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn me_requires_a_valid_session(pool: PgPool) {
    let app = app(pool);
    register(&app, "alice").await;
    let cookie = login_cookie(&app, "alice").await;

    let ok = get(&app, "/api/v1/auth/me", Some(&cookie)).await;
    assert_eq!(ok.status, StatusCode::OK);
    assert_eq!(ok.json["user"]["username"], "alice");

    let none = get(&app, "/api/v1/auth/me", None).await;
    assert_eq!(none.status, StatusCode::UNAUTHORIZED);
    assert_eq!(none.error_code(), "UNAUTHENTICATED");

    let forged = format!("{COOKIE_NAME}=not-a-real-token");
    let bad = get(&app, "/api/v1/auth/me", Some(&forged)).await;
    assert_eq!(bad.status, StatusCode::UNAUTHORIZED);

    // The same token under the wrong cookie name is not accepted.
    let token = cookie.split_once('=').unwrap().1;
    let renamed = format!("rustchat_session={token}");
    let wrong_name = get(&app, "/api/v1/auth/me", Some(&renamed)).await;
    assert_eq!(wrong_name.status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn logout_revokes_the_session_and_clears_the_cookie(pool: PgPool) {
    let app = app(pool);
    register(&app, "alice").await;
    let cookie = login_cookie(&app, "alice").await;

    let reply = authed(&app, Method::POST, "/api/v1/auth/logout", &cookie).await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    assert!(reply.set_cookie_header().contains("Max-Age=0"));

    let after = get(&app, "/api/v1/auth/me", Some(&cookie)).await;
    assert_eq!(after.status, StatusCode::UNAUTHORIZED, "token must be dead");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn state_changing_requests_need_an_allowed_origin(pool: PgPool) {
    let app = app(pool);
    register(&app, "alice").await;
    let cookie = login_cookie(&app, "alice").await;

    for origin in [None, Some("https://evil.example.test"), Some("null")] {
        let reply = send(
            &app,
            Call {
                method: Method::POST,
                uri: "/api/v1/auth/logout",
                body: None,
                cookie: Some(&cookie),
                origin,
            },
        )
        .await;
        assert_eq!(reply.status, StatusCode::FORBIDDEN, "origin {origin:?}");
        assert_eq!(reply.error_code(), "ORIGIN_NOT_ALLOWED");
    }
    // The blocked attempts did not log the user out.
    assert_eq!(
        get(&app, "/api/v1/auth/me", Some(&cookie)).await.status,
        StatusCode::OK
    );

    // Login and register are protected too (login CSRF).
    let cross_site_login = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/auth/login",
            body: Some(json!({"username": "alice", "password": PASSWORD})),
            cookie: None,
            origin: Some("https://evil.example.test"),
        },
    )
    .await;
    assert_eq!(cross_site_login.status, StatusCode::FORBIDDEN);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn sessions_can_be_listed_and_revoked(pool: PgPool) {
    let app = app(pool);
    register(&app, "alice").await;
    let first = login_cookie(&app, "alice").await;
    let second = login_cookie(&app, "alice").await;

    let listed = get(&app, "/api/v1/auth/sessions", Some(&first)).await;
    assert_eq!(listed.status, StatusCode::OK);
    let sessions = listed.json["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions.iter().filter(|s| s["current"] == true).count(), 1);
    let other = sessions.iter().find(|s| s["current"] == false).unwrap();
    let other_id = other["id"].as_str().unwrap().to_owned();

    let uri = format!("/api/v1/auth/sessions/{other_id}");
    let revoked = authed(&app, Method::DELETE, &uri, &first).await;
    assert_eq!(revoked.status, StatusCode::NO_CONTENT);

    // Exactly one of the two tokens died, and it is not the caller's.
    assert_eq!(
        get(&app, "/api/v1/auth/me", Some(&first)).await.status,
        StatusCode::OK
    );
    assert_eq!(
        get(&app, "/api/v1/auth/me", Some(&second)).await.status,
        StatusCode::UNAUTHORIZED
    );

    let again = authed(&app, Method::DELETE, &uri, &first).await;
    assert_eq!(again.status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn password_change_verifies_current_password_and_revokes_other_sessions(pool: PgPool) {
    let app = app(pool);
    register(&app, "alice").await;
    let current = login_cookie(&app, "alice").await;
    let other = login_cookie(&app, "alice").await;

    let wrong = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/auth/password",
            body: Some(json!({"current_password":"incorrect password", "new_password":"new correct horse battery"})),
            cookie: Some(&current),
            origin: Some(ORIGIN),
        },
    ).await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong.error_code(), "INVALID_CREDENTIALS");
    assert_eq!(
        get(&app, "/api/v1/auth/me", Some(&other)).await.status,
        StatusCode::OK
    );

    let invalid_new = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/auth/password",
            body: Some(json!({"current_password":PASSWORD, "new_password":"short"})),
            cookie: Some(&current),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(invalid_new.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        get(&app, "/api/v1/auth/me", Some(&other)).await.status,
        StatusCode::OK
    );

    let changed = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/auth/password",
            body: Some(
                json!({"current_password":PASSWORD, "new_password":"new correct horse battery"}),
            ),
            cookie: Some(&current),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(changed.status, StatusCode::NO_CONTENT);
    assert_eq!(
        get(&app, "/api/v1/auth/me", Some(&current)).await.status,
        StatusCode::OK
    );
    assert_eq!(
        get(&app, "/api/v1/auth/me", Some(&other)).await.status,
        StatusCode::UNAUTHORIZED
    );

    let old_password = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/auth/login",
            body: Some(json!({"username":"alice", "password":PASSWORD})),
            cookie: None,
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(old_password.status, StatusCode::UNAUTHORIZED);
    let new_password = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/auth/login",
            body: Some(json!({"username":"alice", "password":"new correct horse battery"})),
            cookie: None,
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(new_password.status, StatusCode::OK);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn account_lifecycle_revokes_sessions_reactivates_and_retains_deletion_tombstone(
    pool: PgPool,
) {
    let app = app(pool.clone());
    let alice_registration = register(&app, "alice").await;
    register(&app, "bobby").await;
    let alice_id =
        Uuid::parse_str(alice_registration.json["user"]["id"].as_str().unwrap()).unwrap();
    let alice_cookie = login_cookie(&app, "alice").await;
    let bobby_cookie = login_cookie(&app, "bobby").await;
    let owned_community = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/communities",
            body: Some(json!({"name":"Owned space", "description":""})),
            cookie: Some(&alice_cookie),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(owned_community.status, StatusCode::CREATED);
    let blocked_deactivation = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/auth/deactivate",
            body: Some(json!({"current_password":PASSWORD})),
            cookie: Some(&alice_cookie),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(blocked_deactivation.status, StatusCode::CONFLICT);
    assert_eq!(
        blocked_deactivation.error_code(),
        "ACCOUNT_OWNS_COMMUNITIES"
    );
    let community_uri = format!(
        "/api/v1/communities/{}",
        owned_community.json["community"]["id"].as_str().unwrap()
    );
    let removed_community = send(
        &app,
        Call {
            method: Method::DELETE,
            uri: &community_uri,
            body: Some(json!({"confirm":true})),
            cookie: Some(&alice_cookie),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(removed_community.status, StatusCode::NO_CONTENT);
    let direct = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/conversations",
            body: Some(json!({"type":"direct", "username":"bobby"})),
            cookie: Some(&alice_cookie),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(direct.status, StatusCode::CREATED);
    let conversation_id = direct.json["conversation"]["id"].as_str().unwrap();
    let message_uri = format!("/api/v1/conversations/{conversation_id}/messages");
    let message = send(&app, Call {
        method: Method::POST,
        uri: &message_uri,
        body: Some(json!({"client_message_id":"00000000-0000-7000-8000-000000000001", "content":"History survives account deletion"})),
        cookie: Some(&alice_cookie),
        origin: Some(ORIGIN),
    }).await;
    assert_eq!(message.status, StatusCode::CREATED);
    let first = login_cookie(&app, "alice").await;
    let other = login_cookie(&app, "alice").await;
    let deactivated = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/auth/deactivate",
            body: Some(json!({"current_password": PASSWORD})),
            cookie: Some(&first),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(deactivated.status, StatusCode::NO_CONTENT);
    assert_eq!(
        get(&app, "/api/v1/auth/me", Some(&first)).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        get(&app, "/api/v1/auth/me", Some(&other)).await.status,
        StatusCode::UNAUTHORIZED
    );

    let reactivated = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/auth/login",
            body: Some(json!({"username":"alice", "password":PASSWORD})),
            cookie: None,
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(reactivated.status, StatusCode::OK);
    let current = reactivated.session_cookie().unwrap();
    assert_eq!(
        get(&app, "/api/v1/auth/me", Some(&current)).await.status,
        StatusCode::OK
    );

    let unconfirmed = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/auth/delete",
            body: Some(json!({"current_password":PASSWORD, "confirm":false})),
            cookie: Some(&current),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(unconfirmed.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        get(&app, "/api/v1/auth/me", Some(&current)).await.status,
        StatusCode::OK
    );

    let deleted = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/auth/delete",
            body: Some(json!({"current_password":PASSWORD, "confirm":true})),
            cookie: Some(&current),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    assert_eq!(
        get(&app, "/api/v1/auth/me", Some(&current)).await.status,
        StatusCode::UNAUTHORIZED
    );
    let retained_message: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM messages WHERE sender_id=$1 AND conversation_id=$2 AND content='History survives account deletion')")
        .bind(alice_id).bind(Uuid::parse_str(conversation_id).unwrap()).fetch_one(&pool).await.unwrap();
    assert!(retained_message);
    let former_member: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM conversation_members WHERE user_id=$1 AND conversation_id=$2)")
        .bind(alice_id).bind(Uuid::parse_str(conversation_id).unwrap()).fetch_one(&pool).await.unwrap();
    assert!(!former_member);
    let history = get(&app, &message_uri, Some(&bobby_cookie)).await;
    assert_eq!(history.status, StatusCode::OK);
    assert_eq!(
        history.json["messages"][0]["content"],
        "History survives account deletion"
    );
    let tombstone: (
        Option<time::OffsetDateTime>,
        Option<time::OffsetDateTime>,
        String,
    ) = sqlx::query_as(
        "SELECT deactivated_at, deleted_at, username FROM users WHERE deleted_at IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(tombstone.0.is_some() && tombstone.1.is_some());
    assert!(tombstone.2.starts_with("deleted_"));
    let old_login = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/auth/login",
            body: Some(json!({"username":"alice", "password":PASSWORD})),
            cookie: None,
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(old_login.status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn a_user_cannot_see_or_revoke_another_users_sessions(pool: PgPool) {
    let app = app(pool);
    register(&app, "alice").await;
    register(&app, "bobby").await;
    let alice = login_cookie(&app, "alice").await;
    let bobby = login_cookie(&app, "bobby").await;

    let alice_sessions = get(&app, "/api/v1/auth/sessions", Some(&alice)).await;
    let alice_session_id = alice_sessions.json["sessions"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let bobby_view = get(&app, "/api/v1/auth/sessions", Some(&bobby)).await;
    let bobby_sessions = bobby_view.json["sessions"].as_array().unwrap();
    assert_eq!(bobby_sessions.len(), 1);
    assert_ne!(bobby_sessions[0]["id"], alice_session_id.as_str());

    let uri = format!("/api/v1/auth/sessions/{alice_session_id}");
    let attempt = authed(&app, Method::DELETE, &uri, &bobby).await;
    assert_eq!(attempt.status, StatusCode::NOT_FOUND);
    assert_eq!(
        get(&app, "/api/v1/auth/me", Some(&alice)).await.status,
        StatusCode::OK,
        "alice's session must survive"
    );

    let garbage = authed(
        &app,
        Method::DELETE,
        "/api/v1/auth/sessions/not-a-uuid",
        &bobby,
    )
    .await;
    assert_eq!(garbage.status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn an_expired_session_is_rejected(pool: PgPool) {
    let app = app(pool.clone());
    register(&app, "alice").await;
    let cookie = login_cookie(&app, "alice").await;
    assert_eq!(
        get(&app, "/api/v1/auth/me", Some(&cookie)).await.status,
        StatusCode::OK
    );

    // Move expiry into the past while keeping `expires_at > created_at`.
    sqlx::query(
        "UPDATE sessions SET created_at = now() - interval '2 days', \
         expires_at = now() - interval '1 day'",
    )
    .execute(&pool)
    .await
    .unwrap();

    assert_eq!(
        get(&app, "/api/v1/auth/me", Some(&cookie)).await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn secrets_are_not_stored_in_the_clear(pool: PgPool) {
    let app = app(pool.clone());
    register(&app, "alice").await;
    let cookie = login_cookie(&app, "alice").await;
    let token = cookie.split_once('=').unwrap().1.to_owned();

    let hash: String = sqlx::query_scalar("SELECT password_hash FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(hash.starts_with("$argon2id$"), "{hash}");
    assert!(!hash.contains(PASSWORD));

    let digest: Vec<u8> = sqlx::query_scalar("SELECT token_digest FROM sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(digest.len(), 32);
    assert_ne!(digest, token.as_bytes(), "raw token must not be persisted");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn malformed_and_oversized_bodies_get_structured_errors(pool: PgPool) {
    let app = app(pool);

    let not_json = send_raw(&app, "/api/v1/auth/login", "{not json", "application/json").await;
    assert_eq!(not_json.status, StatusCode::BAD_REQUEST);
    assert_eq!(not_json.error_code(), "BAD_REQUEST");

    let wrong_type = send_raw(&app, "/api/v1/auth/login", "{}", "text/plain").await;
    assert_eq!(wrong_type.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(wrong_type.error_code(), "BAD_REQUEST");

    let missing_field = post_json(&app, "/api/v1/auth/login", json!({"username": "alice"})).await;
    assert!(missing_field.status.is_client_error());
    assert_eq!(missing_field.error_code(), "BAD_REQUEST");

    let unknown_field = post_json(
        &app,
        "/api/v1/auth/login",
        json!({"username": "alice", "password": PASSWORD, "admin": true}),
    )
    .await;
    assert_eq!(unknown_field.error_code(), "BAD_REQUEST");

    let huge = json!({"username": "alice", "password": "x".repeat(64 * 1024)});
    let too_large = post_json(&app, "/api/v1/auth/login", huge).await;
    assert_eq!(too_large.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(too_large.error_code(), "PAYLOAD_TOO_LARGE");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn unknown_api_routes_return_a_structured_404(pool: PgPool) {
    let reply = get(&app(pool), "/api/v1/nope", None).await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert_eq!(reply.error_code(), "NOT_FOUND");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn direct_conversations_are_created_once_and_hidden_from_non_members(pool: PgPool) {
    let app = app(pool);
    register(&app, "alice").await;
    register(&app, "bobby").await;
    register(&app, "carol").await;
    register(&app, "david").await;
    let alice = login_cookie(&app, "alice").await;
    let bobby = login_cookie(&app, "bobby").await;
    let david = login_cookie(&app, "david").await;

    let first = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/conversations",
            body: Some(json!({"type":"direct", "username":"Bobby"})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(first.status, StatusCode::CREATED, "{:?}", first.json);
    let again = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/conversations",
            body: Some(json!({"type":"direct", "username":"bobby"})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(again.status, StatusCode::CREATED);
    assert_eq!(
        again.json["conversation"]["id"],
        first.json["conversation"]["id"]
    );
    let id = first.json["conversation"]["id"].as_str().unwrap();

    let second = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/conversations",
            body: Some(json!({"type":"direct", "username":"carol"})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(second.status, StatusCode::CREATED);

    let page_one = get(&app, "/api/v1/conversations?limit=1", Some(&alice)).await;
    assert_eq!(page_one.status, StatusCode::OK);
    assert_eq!(page_one.json["conversations"].as_array().unwrap().len(), 1);
    let cursor = page_one.json["next_cursor"].as_str().unwrap();
    let page_two = get(
        &app,
        &format!("/api/v1/conversations?limit=1&before={cursor}"),
        Some(&alice),
    )
    .await;
    assert_eq!(page_two.status, StatusCode::OK);
    assert_eq!(page_two.json["conversations"].as_array().unwrap().len(), 1);
    assert_ne!(
        page_one.json["conversations"][0]["id"],
        page_two.json["conversations"][0]["id"]
    );

    let hidden = get(&app, &format!("/api/v1/conversations/{id}"), Some(&david)).await;
    assert_eq!(hidden.status, StatusCode::NOT_FOUND);
    let visible = get(&app, &format!("/api/v1/conversations/{id}"), Some(&bobby)).await;
    assert_eq!(visible.status, StatusCode::OK);
    assert_eq!(
        visible.json["conversation"]["members"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let member_profiles = get(
        &app,
        &format!("/api/v1/conversations/{id}/members"),
        Some(&bobby),
    )
    .await;
    assert_eq!(member_profiles.status, StatusCode::OK);
    let usernames = member_profiles.json["members"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|member| member["username"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(usernames, vec!["alice", "bobby"]);
    let hidden_profiles = get(
        &app,
        &format!("/api/v1/conversations/{id}/members"),
        Some(&david),
    )
    .await;
    assert_eq!(hidden_profiles.status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn group_membership_is_scoped_and_respects_the_configured_cap(pool: PgPool) {
    let app = app_with_member_limit(pool, RegistrationPolicy::Open, 3);
    for username in ["alice", "bobby", "carol", "david"] {
        register(&app, username).await;
    }
    let alice = login_cookie(&app, "alice").await;
    let david = login_cookie(&app, "david").await;
    let bobby = login_cookie(&app, "bobby").await;
    let carol = login_cookie(&app, "carol").await;
    let group = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/conversations",
            body: Some(json!({"type":"group", "usernames":["bobby", "carol"]})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(group.status, StatusCode::CREATED, "{:?}", group.json);
    let id = group.json["conversation"]["id"].as_str().unwrap();

    let over_limit = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/conversations",
            body: Some(json!({"type":"group", "usernames":["bobby", "carol", "david"]})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(over_limit.status, StatusCode::UNPROCESSABLE_ENTITY);

    let add = send(
        &app,
        Call {
            method: Method::POST,
            uri: &format!("/api/v1/conversations/{id}/members"),
            body: Some(json!({"username":"david"})),
            cookie: Some(&carol),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(
        add.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "member cap should apply to additions"
    );

    let denied = send(
        &app,
        Call {
            method: Method::POST,
            uri: &format!("/api/v1/conversations/{id}/members"),
            body: Some(json!({"username":"david"})),
            cookie: Some(&david),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(
        denied.status,
        StatusCode::NOT_FOUND,
        "non-members cannot add members"
    );

    let bobby_id = get(&app, "/api/v1/auth/me", Some(&bobby)).await.json["user"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let remove = authed(
        &app,
        Method::DELETE,
        &format!("/api/v1/conversations/{id}/members/{bobby_id}"),
        &alice,
    )
    .await;
    assert_eq!(remove.status, StatusCode::NO_CONTENT);
    let add = send(
        &app,
        Call {
            method: Method::POST,
            uri: &format!("/api/v1/conversations/{id}/members"),
            body: Some(json!({"username":"david"})),
            cookie: Some(&carol),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(add.status, StatusCode::NO_CONTENT);
    assert_eq!(
        get(&app, &format!("/api/v1/conversations/{id}"), Some(&david))
            .await
            .status,
        StatusCode::OK
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn community_invites_channels_and_roles_enforce_membership_permissions(pool: PgPool) {
    let app = app(pool.clone());
    for username in ["alice", "bobby", "carol"] {
        register(&app, username).await;
    }
    let alice = login_cookie(&app, "alice").await;
    let bobby = login_cookie(&app, "bobby").await;
    let carol = login_cookie(&app, "carol").await;

    let created = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/communities",
            body: Some(json!({"name":"Rustaceans", "description":"A place to build"})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.json);
    assert!(created.json["community"]["created_at"].is_string());
    let community_id = created.json["community"]["id"].as_str().unwrap();
    let categories_uri = format!("/api/v1/communities/{community_id}/categories");
    let category = send(
        &app,
        Call {
            method: Method::POST,
            uri: &categories_uri,
            body: Some(json!({"name":"Projects"})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(category.status, StatusCode::CREATED, "{:?}", category.json);
    let category_id = category.json["category"]["id"].as_str().unwrap();
    let second_category = send(
        &app,
        Call {
            method: Method::POST,
            uri: &categories_uri,
            body: Some(json!({"name":"General"})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(second_category.status, StatusCode::CREATED);
    let second_category_id = second_category.json["category"]["id"].as_str().unwrap();
    let category_update = send(
        &app,
        Call {
            method: Method::PATCH,
            uri: &format!("{categories_uri}/{second_category_id}"),
            body: Some(json!({"name":"Lobby","position":0})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(
        category_update.status,
        StatusCode::OK,
        "{:?}",
        category_update.json
    );
    assert_eq!(category_update.json["category"]["name"], "Lobby");
    let listed_categories = get(&app, &categories_uri, Some(&alice)).await;
    assert_eq!(listed_categories.json["categories"][0]["name"], "Lobby");
    let hidden_categories = get(&app, &categories_uri, Some(&carol)).await;
    assert_eq!(hidden_categories.status, StatusCode::NOT_FOUND);
    let channels_uri = format!("/api/v1/communities/{community_id}/channels");
    let channel = send(
        &app,
        Call {
            method: Method::POST,
            uri: &channels_uri,
            body: Some(json!({"name":"general", "description":"Open discussion", "category_id":category_id})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(channel.status, StatusCode::CREATED, "{:?}", channel.json);
    assert!(channel.json["channel"]["created_at"].is_string());
    assert_eq!(channel.json["channel"]["category_id"], category_id);
    let channel_id = channel.json["channel"]["id"].as_str().unwrap();
    let channel_uri = format!("{channels_uri}/{channel_id}");
    let moved_channel = send(&app, Call {
        method: Method::PATCH,
        uri: &channel_uri,
        body: Some(json!({"name":"general","description":"Open discussion","read_only":false,"position":0,"category_id":second_category_id})),
        cookie: Some(&alice),
        origin: Some(ORIGIN),
    }).await;
    assert_eq!(
        moved_channel.status,
        StatusCode::OK,
        "{:?}",
        moved_channel.json
    );
    assert_eq!(
        moved_channel.json["channel"]["category_id"],
        second_category_id
    );
    let unchanged_category = send(&app, Call {
        method: Method::PATCH,
        uri: &channel_uri,
        body: Some(json!({"name":"general","description":"Open discussion","read_only":false,"position":0})),
        cookie: Some(&alice),
        origin: Some(ORIGIN),
    }).await;
    assert_eq!(
        unchanged_category.json["channel"]["category_id"],
        second_category_id
    );
    let deleted_category = send(
        &app,
        Call {
            method: Method::DELETE,
            uri: &format!("{categories_uri}/{second_category_id}"),
            body: None,
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(deleted_category.status, StatusCode::NO_CONTENT);
    let refreshed_channels = get(&app, &channels_uri, Some(&alice)).await;
    assert_eq!(
        refreshed_channels.json["channels"][0]["category_id"],
        serde_json::Value::Null
    );
    let conversation_id = channel.json["channel"]["conversation_id"].as_str().unwrap();
    let second_channel = send(
        &app,
        Call {
            method: Method::POST,
            uri: &channels_uri,
            body: Some(json!({"name":"announcements"})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(second_channel.status, StatusCode::CREATED);
    let second_channel_id = second_channel.json["channel"]["id"].as_str().unwrap();
    let second_channel_uri =
        format!("/api/v1/communities/{community_id}/channels/{second_channel_id}");
    let channel_update = send(
        &app,
        Call {
            method: Method::PATCH,
            uri: &second_channel_uri,
            body: Some(
                json!({"name":"news","description":"Updates","read_only":false,"position":0}),
            ),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(
        channel_update.status,
        StatusCode::OK,
        "{:?}",
        channel_update.json
    );
    assert_eq!(channel_update.json["channel"]["name"], "news");
    assert_eq!(channel_update.json["channel"]["position"], 0);
    assert_eq!(
        send(
            &app,
            Call {
                method: Method::DELETE,
                uri: &second_channel_uri,
                body: None,
                cookie: Some(&bobby),
                origin: Some(ORIGIN)
            }
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(&app, &channels_uri, Some(&bobby)).await.status,
        StatusCode::NOT_FOUND
    );

    let invite_uri = format!("/api/v1/communities/{community_id}/invites");
    let invite = send(
        &app,
        Call {
            method: Method::POST,
            uri: &invite_uri,
            body: Some(json!({"max_uses":1, "ttl_seconds":600})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(invite.status, StatusCode::CREATED, "{:?}", invite.json);
    let listed_invites = get(&app, &invite_uri, Some(&alice)).await;
    assert_eq!(
        listed_invites.status,
        StatusCode::OK,
        "{:?}",
        listed_invites.json
    );
    assert_eq!(listed_invites.json["invites"].as_array().unwrap().len(), 1);
    assert!(listed_invites.json["invites"][0].get("id").is_some());
    assert!(
        listed_invites.json["invites"][0]
            .get("invite_code")
            .is_none()
    );
    assert_eq!(
        get(&app, &invite_uri, Some(&carol)).await.status,
        StatusCode::NOT_FOUND
    );
    let joined = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/communities/join",
            body: Some(json!({"invite_code":invite.json["invite_code"]})),
            cookie: Some(&bobby),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(joined.status, StatusCode::OK, "{:?}", joined.json);
    assert_eq!(
        get(&app, &channels_uri, Some(&bobby)).await.status,
        StatusCode::OK
    );
    let carol_invite = send(
        &app,
        Call {
            method: Method::POST,
            uri: &invite_uri,
            body: Some(json!({})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(carol_invite.status, StatusCode::CREATED);
    assert_eq!(
        send(
            &app,
            Call {
                method: Method::POST,
                uri: "/api/v1/communities/join",
                body: Some(json!({"invite_code":carol_invite.json["invite_code"]})),
                cookie: Some(&carol),
                origin: Some(ORIGIN),
            }
        )
        .await
        .status,
        StatusCode::OK
    );
    let carol_id = get(&app, "/api/v1/auth/me", Some(&carol)).await.json["user"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let carol_membership_uri = format!("/api/v1/communities/{community_id}/members/{carol_id}");
    assert_eq!(
        send(
            &app,
            Call {
                method: Method::DELETE,
                uri: &carol_membership_uri,
                body: None,
                cookie: Some(&bobby),
                origin: Some(ORIGIN)
            }
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(
            &app,
            Call {
                method: Method::DELETE,
                uri: &carol_membership_uri,
                body: None,
                cookie: Some(&alice),
                origin: Some(ORIGIN)
            }
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        get(&app, &channels_uri, Some(&carol)).await.status,
        StatusCode::NOT_FOUND
    );
    let remaining_members = get(
        &app,
        &format!("/api/v1/communities/{community_id}/members"),
        Some(&alice),
    )
    .await;
    assert!(
        !remaining_members.json["members"]
            .as_array()
            .unwrap()
            .iter()
            .any(|member| member["id"] == carol_id)
    );
    let invite_reuse = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/communities/join",
            body: Some(json!({"invite_code":invite.json["invite_code"]})),
            cookie: Some(&carol),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(invite_reuse.status, StatusCode::NOT_FOUND);

    let role_uri = format!("/api/v1/communities/{community_id}/roles");
    let role = send(
        &app,
        Call {
            method: Method::POST,
            uri: &role_uri,
            body: Some(json!({"name":"restricted", "permissions":28})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(role.status, StatusCode::CREATED, "{:?}", role.json);
    let role_id = role.json["role"]["id"].as_str().unwrap();
    let role_uri = format!("{role_uri}/{role_id}");
    let role_update = send(
        &app,
        Call {
            method: Method::PATCH,
            uri: &role_uri,
            body: Some(json!({"name":"moderators","permissions":28,"position":1})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(role_update.status, StatusCode::OK, "{:?}", role_update.json);
    assert_eq!(role_update.json["role"]["name"], "moderators");
    let bobby_profile = get(&app, "/api/v1/auth/me", Some(&bobby)).await;
    let bobby_id = bobby_profile.json["user"]["id"].as_str().unwrap();
    let assign_uri =
        format!("/api/v1/communities/{community_id}/members/{bobby_id}/roles/{role_id}");
    assert_eq!(
        send(
            &app,
            Call {
                method: Method::POST,
                uri: &assign_uri,
                body: None,
                cookie: Some(&alice),
                origin: Some(ORIGIN),
            }
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    let alice_id = get(&app, "/api/v1/auth/me", Some(&alice)).await.json["user"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        send(
            &app,
            Call {
                method: Method::POST,
                uri: &format!(
                    "/api/v1/communities/{community_id}/members/{alice_id}/roles/{role_id}"
                ),
                body: None,
                cookie: Some(&alice),
                origin: Some(ORIGIN),
            }
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    let owner_role_update = send(
        &app,
        Call {
            method: Method::PATCH,
            uri: &role_uri,
            body: Some(json!({"name":"moderators","permissions":28,"position":1})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(owner_role_update.status, StatusCode::OK);
    let subordinate = send(
        &app,
        Call {
            method: Method::POST,
            uri: &format!("/api/v1/communities/{community_id}/roles"),
            body: Some(json!({"name":"helpers", "permissions":1})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    let subordinate_id = subordinate.json["role"]["id"].as_str().unwrap();
    let move_above_manager = send(
        &app,
        Call {
            method: Method::PATCH,
            uri: &format!("/api/v1/communities/{community_id}/roles/{subordinate_id}"),
            body: Some(json!({"name":"helpers", "permissions":1, "position":1})),
            cookie: Some(&bobby),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(move_above_manager.status, StatusCode::NOT_FOUND);
    let lower_role = send(
        &app,
        Call {
            method: Method::POST,
            uri: &format!("/api/v1/communities/{community_id}/roles"),
            body: Some(json!({"name":"newbies", "permissions":1})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    let lower_role_id = lower_role.json["role"]["id"].as_str().unwrap();
    let escalate_permissions = send(
        &app,
        Call {
            method: Method::PATCH,
            uri: &format!("/api/v1/communities/{community_id}/roles/{lower_role_id}"),
            body: Some(json!({"name":"newbies", "permissions":32, "position":3})),
            cookie: Some(&bobby),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(escalate_permissions.status, StatusCode::NOT_FOUND);
    let manage_lower_role = send(
        &app,
        Call {
            method: Method::PATCH,
            uri: &format!("/api/v1/communities/{community_id}/roles/{lower_role_id}"),
            body: Some(json!({"name":"newbies-updated", "permissions":1, "position":3})),
            cookie: Some(&bobby),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(manage_lower_role.status, StatusCode::OK);
    let delete_lower_role = send(
        &app,
        Call {
            method: Method::DELETE,
            uri: &format!("/api/v1/communities/{community_id}/roles/{lower_role_id}"),
            body: None,
            cookie: Some(&bobby),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(delete_lower_role.status, StatusCode::NO_CONTENT);
    let assign_equal_rank = send(
        &app,
        Call {
            method: Method::POST,
            uri: &format!("/api/v1/communities/{community_id}/members/{carol_id}/roles/{role_id}"),
            body: None,
            cookie: Some(&bobby),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(assign_equal_rank.status, StatusCode::NOT_FOUND);
    let remove_equal_rank = send(
        &app,
        Call {
            method: Method::DELETE,
            uri: &format!("/api/v1/communities/{community_id}/members/{bobby_id}/roles/{role_id}"),
            body: None,
            cookie: Some(&bobby),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(remove_equal_rank.status, StatusCode::NOT_FOUND);
    let delete_equal_rank = send(
        &app,
        Call {
            method: Method::DELETE,
            uri: &role_uri,
            body: None,
            cookie: Some(&bobby),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(delete_equal_rank.status, StatusCode::NOT_FOUND);
    let override_uri = format!("/api/v1/channels/{channel_id}/permissions/{role_id}");
    let conflicting_override = send(
        &app,
        Call {
            method: Method::PUT,
            uri: &override_uri,
            body: Some(json!({"allow":64,"deny":64})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(
        conflicting_override.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        send(
            &app,
            Call {
                method: Method::PUT,
                uri: &override_uri,
                body: Some(json!({"allow":0,"deny":64})),
                cookie: Some(&alice),
                origin: Some(ORIGIN),
            }
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    let denied_send = send(
        &app,
        Call {
            method: Method::POST,
            uri: &format!("/api/v1/conversations/{conversation_id}/messages"),
            body: Some(json!({"client_message_id":Uuid::now_v7(), "content":"not permitted"})),
            cookie: Some(&bobby),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(denied_send.status, StatusCode::NOT_FOUND);
    assert_eq!(
        send(
            &app,
            Call {
                method: Method::DELETE,
                uri: &assign_uri,
                body: None,
                cookie: Some(&alice),
                origin: Some(ORIGIN)
            }
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            &app,
            Call {
                method: Method::DELETE,
                uri: &role_uri,
                body: None,
                cookie: Some(&alice),
                origin: Some(ORIGIN)
            }
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            &app,
            Call {
                method: Method::DELETE,
                uri: &second_channel_uri,
                body: None,
                cookie: Some(&alice),
                origin: Some(ORIGIN)
            }
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        get(
            &app,
            &format!(
                "/api/v1/conversations/{}",
                second_channel.json["channel"]["conversation_id"]
                    .as_str()
                    .unwrap()
            ),
            Some(&alice)
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );

    let revoked_invite = send(
        &app,
        Call {
            method: Method::POST,
            uri: &invite_uri,
            body: Some(json!({"ttl_seconds":600})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(revoked_invite.status, StatusCode::CREATED);
    let revoke_body = Some(json!({"invite_code":revoked_invite.json["invite_code"]}));
    assert_eq!(
        send(
            &app,
            Call {
                method: Method::DELETE,
                uri: &invite_uri,
                body: revoke_body.clone(),
                cookie: Some(&bobby),
                origin: Some(ORIGIN)
            }
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(
            &app,
            Call {
                method: Method::DELETE,
                uri: &invite_uri,
                body: revoke_body,
                cookie: Some(&alice),
                origin: Some(ORIGIN)
            }
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            &app,
            Call {
                method: Method::POST,
                uri: "/api/v1/communities/join",
                body: Some(json!({"invite_code":revoked_invite.json["invite_code"]})),
                cookie: Some(&carol),
                origin: Some(ORIGIN)
            }
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    let _active_invite = send(
        &app,
        Call {
            method: Method::POST,
            uri: &invite_uri,
            body: Some(json!({})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(_active_invite.status, StatusCode::CREATED);
    let listed_invite = get(&app, &invite_uri, Some(&alice)).await;
    let listed_invite_id = listed_invite.json["invites"][0]["id"].as_str().unwrap();
    let revoke_by_id_uri = format!("{invite_uri}/{listed_invite_id}");
    assert_eq!(
        send(
            &app,
            Call {
                method: Method::DELETE,
                uri: &revoke_by_id_uri,
                body: None,
                cookie: Some(&alice),
                origin: Some(ORIGIN)
            }
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    let updated_invites = get(&app, &invite_uri, Some(&alice)).await;
    assert!(
        updated_invites.json["invites"]
            .as_array()
            .unwrap()
            .iter()
            .any(|invite| invite["id"] == listed_invite_id && !invite["revoked_at"].is_null())
    );
    let leave_uri = format!("/api/v1/communities/{community_id}/members");
    assert_eq!(
        send(
            &app,
            Call {
                method: Method::DELETE,
                uri: &leave_uri,
                body: None,
                cookie: Some(&alice),
                origin: Some(ORIGIN)
            }
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        send(
            &app,
            Call {
                method: Method::DELETE,
                uri: &leave_uri,
                body: None,
                cookie: Some(&bobby),
                origin: Some(ORIGIN)
            }
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        get(&app, &channels_uri, Some(&bobby)).await.status,
        StatusCode::NOT_FOUND
    );
    let transfer_invite = send(
        &app,
        Call {
            method: Method::POST,
            uri: &invite_uri,
            body: Some(json!({})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(transfer_invite.status, StatusCode::CREATED);
    assert_eq!(
        send(
            &app,
            Call {
                method: Method::POST,
                uri: "/api/v1/communities/join",
                body: Some(json!({"invite_code":transfer_invite.json["invite_code"]})),
                cookie: Some(&bobby),
                origin: Some(ORIGIN)
            }
        )
        .await
        .status,
        StatusCode::OK
    );
    let transfer_uri = format!("/api/v1/communities/{community_id}/ownership");
    assert_eq!(
        send(
            &app,
            Call {
                method: Method::POST,
                uri: &transfer_uri,
                body: Some(json!({"new_owner_id":bobby_id,"confirm":true})),
                cookie: Some(&alice),
                origin: Some(ORIGIN)
            }
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            &app,
            Call {
                method: Method::PATCH,
                uri: &format!("/api/v1/communities/{community_id}"),
                body: Some(json!({"name":"No longer owner","description":""})),
                cookie: Some(&alice),
                origin: Some(ORIGIN)
            }
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    let settings = send(
        &app,
        Call {
            method: Method::PATCH,
            uri: &format!("/api/v1/communities/{community_id}"),
            body: Some(json!({"name":"RustChat","description":"Updated by new owner"})),
            cookie: Some(&bobby),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(settings.status, StatusCode::OK, "{:?}", settings.json);
    let audit_before_delete: i64 = sqlx::query_scalar("SELECT count(*) FROM community_audit_log WHERE community_id=$1 AND event_type IN ('ownership.transferred','settings.updated')").bind(Uuid::parse_str(community_id).unwrap()).fetch_one(&pool).await.unwrap();
    assert_eq!(audit_before_delete, 2);
    let community_uri = format!("/api/v1/communities/{community_id}");
    assert_eq!(
        send(
            &app,
            Call {
                method: Method::DELETE,
                uri: &community_uri,
                body: Some(json!({"confirm":false})),
                cookie: Some(&bobby),
                origin: Some(ORIGIN)
            }
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        send(
            &app,
            Call {
                method: Method::DELETE,
                uri: &community_uri,
                body: Some(json!({"confirm":true})),
                cookie: Some(&bobby),
                origin: Some(ORIGIN)
            }
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    let deletion_audit: i64 = sqlx::query_scalar("SELECT count(*) FROM community_audit_log WHERE community_id IS NULL AND event_type='community.deleted'").fetch_one(&pool).await.unwrap();
    assert_eq!(deletion_audit, 1);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn message_send_is_ordered_idempotent_and_member_scoped(pool: PgPool) {
    let app = app(pool.clone());
    register(&app, "alice").await;
    register(&app, "bobby").await;
    register(&app, "carol").await;
    let alice = login_cookie(&app, "alice").await;
    let bobby = login_cookie(&app, "bobby").await;
    let carol = login_cookie(&app, "carol").await;

    let direct = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/conversations",
            body: Some(json!({"type":"direct", "username":"bobby"})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(direct.status, StatusCode::CREATED);
    let conversation_id = direct.json["conversation"]["id"].as_str().unwrap();
    let send_uri = format!("/api/v1/conversations/{conversation_id}/messages");
    let client_id = Uuid::now_v7();

    let first = send(
        &app,
        Call {
            method: Method::POST,
            uri: &send_uri,
            body: Some(json!({"client_message_id":client_id, "content":"hello"})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(first.status, StatusCode::CREATED, "{:?}", first.json);
    assert_eq!(first.json["message"]["sequence"], 1);
    let retry = send(
        &app,
        Call {
            method: Method::POST,
            uri: &send_uri,
            body: Some(json!({"client_message_id":client_id, "content":"hello"})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(retry.status, StatusCode::CREATED);
    assert_eq!(retry.json["message"]["id"], first.json["message"]["id"]);
    assert_eq!(retry.json["message"]["sequence"], 1);

    let conflict = send(
        &app,
        Call {
            method: Method::POST,
            uri: &send_uri,
            body: Some(json!({"client_message_id":client_id, "content":"different"})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(conflict.status, StatusCode::CONFLICT);
    assert_eq!(conflict.error_code(), "IDEMPOTENCY_CONFLICT");

    let second = send(
        &app,
        Call {
            method: Method::POST,
            uri: &send_uri,
            body: Some(json!({"client_message_id":Uuid::now_v7(), "content":"world"})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(second.status, StatusCode::CREATED);
    assert_eq!(second.json["message"]["sequence"], 2);

    let delivery_cursor_uri = format!("/api/v1/conversations/{conversation_id}/delivery-cursor");
    let delivery_cursors_uri = format!("/api/v1/conversations/{conversation_id}/delivery-cursors");
    let delivered = send(
        &app,
        Call {
            method: Method::PUT,
            uri: &delivery_cursor_uri,
            body: Some(json!({"sequence":2})),
            cookie: Some(&bobby),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(delivered.status, StatusCode::OK, "{:?}", delivered.json);
    assert_eq!(delivered.json["cursor"]["sequence"], 2);
    let regressed_delivery = send(
        &app,
        Call {
            method: Method::PUT,
            uri: &delivery_cursor_uri,
            body: Some(json!({"sequence":1})),
            cookie: Some(&bobby),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(regressed_delivery.json["cursor"]["sequence"], 2);
    let visible_delivery = get(&app, &delivery_cursors_uri, Some(&alice)).await;
    assert_eq!(visible_delivery.status, StatusCode::OK);
    assert!(
        visible_delivery.json["cursors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|cursor| cursor["sequence"] == 2)
    );
    assert_eq!(
        send(
            &app,
            Call {
                method: Method::PUT,
                uri: &delivery_cursor_uri,
                body: Some(json!({"sequence":3})),
                cookie: Some(&bobby),
                origin: Some(ORIGIN),
            },
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        get(&app, &delivery_cursors_uri, Some(&carol)).await.status,
        StatusCode::NOT_FOUND
    );

    let first_message_id = first.json["message"]["id"].as_str().unwrap();
    let reaction_uri =
        format!("/api/v1/conversations/{conversation_id}/messages/{first_message_id}/reactions");
    let alice_reaction = send(&app, add_reaction_call(&reaction_uri, &alice)).await;
    assert_eq!(alice_reaction.status, StatusCode::OK);
    assert!(alice_reaction.json["changed"].as_bool().unwrap());
    assert_eq!(alice_reaction.json["reactions"][0]["count"], 1);
    let duplicate_reaction = send(&app, add_reaction_call(&reaction_uri, &alice)).await;
    assert_eq!(duplicate_reaction.status, StatusCode::OK);
    assert!(!duplicate_reaction.json["changed"].as_bool().unwrap());

    let bobby_reaction = send(&app, add_reaction_call(&reaction_uri, &bobby)).await;
    assert_eq!(bobby_reaction.status, StatusCode::OK);
    assert_eq!(bobby_reaction.json["reactions"][0]["count"], 2);
    let unsupported_reaction = send(
        &app,
        Call {
            method: Method::PUT,
            uri: &reaction_uri,
            body: Some(json!({"emoji":"not emoji"})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(
        unsupported_reaction.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(unsupported_reaction.error_code(), "INVALID_REACTION");
    let reaction_list = get(
        &app,
        &format!(
            "/api/v1/conversations/{conversation_id}/reactions?message_ids={first_message_id}"
        ),
        Some(&alice),
    )
    .await;
    assert_eq!(reaction_list.status, StatusCode::OK);
    assert_eq!(reaction_list.json["reactions"][0]["count"], 2);
    assert!(
        reaction_list.json["reactions"][0]["reacted_by_me"]
            .as_bool()
            .unwrap()
    );
    assert_eq!(
        get(
            &app,
            &format!(
                "/api/v1/conversations/{conversation_id}/reactions?message_ids={first_message_id}"
            ),
            Some(&carol)
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );

    let removed_reaction = send(
        &app,
        Call {
            method: Method::DELETE,
            uri: &reaction_uri,
            body: Some(json!({"emoji":"👍"})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(removed_reaction.status, StatusCode::OK);
    assert_eq!(removed_reaction.json["reactions"][0]["count"], 1);
    assert!(
        !removed_reaction.json["reactions"][0]["reacted_by_me"]
            .as_bool()
            .unwrap()
    );
    assert_eq!(
        send(&app, add_reaction_call(&reaction_uri, &carol))
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    let read_cursor_uri = format!("/api/v1/conversations/{conversation_id}/read-cursor");
    let read_cursors_uri = format!("/api/v1/conversations/{conversation_id}/read-cursors");
    let advanced = send(
        &app,
        Call {
            method: Method::PUT,
            uri: &read_cursor_uri,
            body: Some(json!({"sequence": 2})),
            cookie: Some(&bobby),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(advanced.status, StatusCode::OK, "{:?}", advanced.json);
    assert_eq!(advanced.json["cursor"]["sequence"], 2);
    let visible_cursor = get(&app, &read_cursors_uri, Some(&alice)).await;
    assert_eq!(visible_cursor.status, StatusCode::OK);
    assert_eq!(visible_cursor.json["cursors"].as_array().unwrap().len(), 2);
    let bobby_id = get(&app, "/api/v1/auth/me", Some(&bobby)).await.json["user"]["id"].clone();
    assert!(
        visible_cursor.json["cursors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|cursor| cursor["user_id"] == bobby_id && cursor["sequence"] == 2)
    );
    let regressed = send(
        &app,
        Call {
            method: Method::PUT,
            uri: &read_cursor_uri,
            body: Some(json!({"sequence": 1})),
            cookie: Some(&bobby),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(regressed.json["cursor"]["sequence"], 2);
    let beyond_latest = send(
        &app,
        Call {
            method: Method::PUT,
            uri: &read_cursor_uri,
            body: Some(json!({"sequence": 3})),
            cookie: Some(&bobby),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(beyond_latest.status, StatusCode::UNPROCESSABLE_ENTITY);
    let hidden_cursors = get(&app, &read_cursors_uri, Some(&carol)).await;
    assert_eq!(hidden_cursors.status, StatusCode::NOT_FOUND);

    let page_one = get(&app, &format!("{send_uri}?limit=1"), Some(&bobby)).await;
    assert_eq!(page_one.status, StatusCode::OK);
    assert_eq!(page_one.json["messages"][0]["sequence"], 2);
    assert_eq!(page_one.json["next_before"], 2);
    let page_two = get(
        &app,
        &format!("{send_uri}?limit=1&before_sequence=2"),
        Some(&bobby),
    )
    .await;
    assert_eq!(page_two.status, StatusCode::OK);
    assert_eq!(page_two.json["messages"][0]["sequence"], 1);
    assert_eq!(page_two.json["messages"][0]["content"], "hello");
    let reconnect = get(
        &app,
        &format!("{send_uri}?after_sequence=1&limit=10"),
        Some(&bobby),
    )
    .await;
    assert_eq!(reconnect.status, StatusCode::OK);
    assert_eq!(reconnect.json["messages"].as_array().unwrap().len(), 1);
    assert_eq!(reconnect.json["messages"][0]["sequence"], 2);

    let message_id = first.json["message"]["id"].as_str().unwrap();
    let message_uri = format!("{send_uri}/{message_id}");
    let forbidden_edit = send(
        &app,
        Call {
            method: Method::PATCH,
            uri: &message_uri,
            body: Some(json!({"content":"intrusion"})),
            cookie: Some(&bobby),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(forbidden_edit.status, StatusCode::NOT_FOUND);
    let edited = send(
        &app,
        Call {
            method: Method::PATCH,
            uri: &message_uri,
            body: Some(json!({"content":"hello edited"})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(edited.status, StatusCode::OK, "{:?}", edited.json);
    assert_eq!(edited.json["message"]["content"], "hello edited");
    assert!(edited.json["message"]["edited_at"].is_string());
    let edited_again = send(
        &app,
        Call {
            method: Method::PATCH,
            uri: &message_uri,
            body: Some(json!({"content":"hello edited twice"})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(
        edited_again.status,
        StatusCode::OK,
        "{:?}",
        edited_again.json
    );
    assert_eq!(
        edited_again.json["message"]["content"],
        "hello edited twice"
    );
    let deleted = send(
        &app,
        Call {
            method: Method::DELETE,
            uri: &message_uri,
            body: None,
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(deleted.status, StatusCode::OK);
    assert_eq!(deleted.json["message"]["content"], "[deleted]");
    assert!(deleted.json["message"]["deleted_at"].is_string());
    let edit_outbox_count: i64 = sqlx::query_scalar("SELECT count(*) FROM outbox_events WHERE conversation_id = $1 AND event_type IN ('message.updated', 'message.deleted')")
        .bind(Uuid::parse_str(conversation_id).unwrap()).fetch_one(&pool).await.unwrap();
    assert_eq!(edit_outbox_count, 3);

    let denied = send(
        &app,
        Call {
            method: Method::POST,
            uri: &send_uri,
            body: Some(json!({"client_message_id":Uuid::now_v7(), "content":"intrusion"})),
            cookie: Some(&carol),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(denied.status, StatusCode::NOT_FOUND);
    assert_eq!(
        get(&app, &format!("{send_uri}?limit=10"), Some(&carol))
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    let outbox_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM outbox_events WHERE conversation_id = $1")
            .bind(Uuid::parse_str(conversation_id).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        outbox_count, 5,
        "message creation and updates create outbox records"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn message_replies_are_durable_conversation_scoped_and_idempotent(pool: PgPool) {
    let app = app(pool.clone());
    register(&app, "alice").await;
    register(&app, "bobby").await;
    register(&app, "carol").await;
    let alice = login_cookie(&app, "alice").await;
    let bobby = login_cookie(&app, "bobby").await;
    let carol = login_cookie(&app, "carol").await;
    let direct = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/conversations",
            body: Some(json!({"type":"direct", "username":"bobby"})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    let conversation_id = direct.json["conversation"]["id"].as_str().unwrap();
    let outsider_conversation = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/conversations",
            body: Some(json!({"type":"direct", "username":"carol"})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    let outsider_conversation_id = outsider_conversation.json["conversation"]["id"]
        .as_str()
        .unwrap();
    let root = send(
        &app,
        Call {
            method: Method::POST,
            uri: &format!("/api/v1/conversations/{conversation_id}/messages"),
            body: Some(json!({"client_message_id":Uuid::now_v7(), "content":"root message"})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    let root_id = root.json["message"]["id"].as_str().unwrap();
    let other_root = send(
        &app,
        Call {
            method: Method::POST,
            uri: &format!("/api/v1/conversations/{conversation_id}/messages"),
            body: Some(json!({"client_message_id":Uuid::now_v7(), "content":"another root"})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    let other_root_id = other_root.json["message"]["id"].as_str().unwrap();
    let foreign_root = send(
        &app,
        Call {
            method: Method::POST,
            uri: &format!("/api/v1/conversations/{outsider_conversation_id}/messages"),
            body: Some(json!({"client_message_id":Uuid::now_v7(), "content":"private root"})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    let reply_client_id = Uuid::now_v7();
    let reply_uri = format!("/api/v1/conversations/{conversation_id}/messages");
    let reply_body = json!({"client_message_id":reply_client_id, "reply_to_message_id":root_id, "content":"a reply"});
    let reply = send(
        &app,
        Call {
            method: Method::POST,
            uri: &reply_uri,
            body: Some(reply_body.clone()),
            cookie: Some(&bobby),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.json);
    assert_eq!(reply.json["message"]["reply_to_message_id"], root_id);

    let retry = send(
        &app,
        Call {
            method: Method::POST,
            uri: &reply_uri,
            body: Some(reply_body),
            cookie: Some(&bobby),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(retry.json["message"]["id"], reply.json["message"]["id"]);

    let conflicting_retry = send(
        &app,
        Call {
            method: Method::POST,
            uri: &reply_uri,
            body: Some(json!({"client_message_id":reply_client_id, "reply_to_message_id":other_root_id, "content":"a reply"})),
            cookie: Some(&bobby),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(conflicting_retry.status, StatusCode::CONFLICT);
    assert_eq!(conflicting_retry.error_code(), "IDEMPOTENCY_CONFLICT");

    let foreign_target = send(
        &app,
        Call {
            method: Method::POST,
            uri: &reply_uri,
            body: Some(json!({"client_message_id":Uuid::now_v7(), "reply_to_message_id":foreign_root.json["message"]["id"], "content":"invalid cross-conversation reply"})),
            cookie: Some(&bobby),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(foreign_target.status, StatusCode::NOT_FOUND);

    let history = get(
        &app,
        &format!("/api/v1/conversations/{conversation_id}/messages?limit=10"),
        Some(&alice),
    )
    .await;
    assert!(
        history.json["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| {
                message["id"] == reply.json["message"]["id"]
                    && message["reply_to_message_id"] == root_id
            })
    );
    let persisted_reply: Option<Uuid> =
        sqlx::query_scalar("SELECT reply_to_message_id FROM messages WHERE id = $1")
            .bind(Uuid::parse_str(reply.json["message"]["id"].as_str().unwrap()).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(persisted_reply, Some(Uuid::parse_str(root_id).unwrap()));
    let outbox_reply: Option<String> = sqlx::query_scalar(
        "SELECT payload->'payload'->>'reply_to_message_id' FROM outbox_events WHERE event_type = 'message.created' AND payload->'payload'->>'message_id' = $1",
    )
    .bind(reply.json["message"]["id"].as_str().unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(outbox_reply.as_deref(), Some(root_id));
    assert_eq!(
        get(&app, "/api/v1/auth/me", Some(&carol)).await.status,
        StatusCode::OK
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn mentions_resolve_only_current_conversation_members_and_follow_edits(pool: PgPool) {
    let app = app(pool.clone());
    for name in ["alice", "bobby", "carol", "david"] {
        register(&app, name).await;
    }
    let alice = login_cookie(&app, "alice").await;
    let david = login_cookie(&app, "david").await;
    let group = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/conversations",
            body: Some(json!({"type":"group", "usernames":["bobby", "carol"]})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    let conversation_id = group.json["conversation"]["id"].as_str().unwrap();
    let david_id = get(&app, "/api/v1/auth/me", Some(&david)).await.json["user"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let members = get(
        &app,
        &format!("/api/v1/conversations/{conversation_id}/members"),
        Some(&alice),
    )
    .await;
    let id_for = |username: &str| {
        members.json["members"]
            .as_array()
            .unwrap()
            .iter()
            .find(|member| member["username"] == username)
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let bobby_id = id_for("bobby");
    let carol_id = id_for("carol");
    let message_uri = format!("/api/v1/conversations/{conversation_id}/messages");
    let sent = send(
        &app,
        Call {
            method: Method::POST,
            uri: &message_uri,
            body: Some(json!({
                "client_message_id":Uuid::now_v7(),
                "content":"Hi @bobby and @CAROL; @david and @unknown. Email: mail@example.com"
            })),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(sent.status, StatusCode::CREATED, "{:?}", sent.json);
    let mentions: Vec<_> = sent.json["message"]["mentioned_user_ids"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(mentions.len(), 2);
    assert!(mentions.contains(&bobby_id.as_str()));
    assert!(mentions.contains(&carol_id.as_str()));
    assert!(!mentions.contains(&david_id.as_str()));

    let message_id = sent.json["message"]["id"].as_str().unwrap();
    let outbox_mentions: Vec<Uuid> = sqlx::query_scalar(
        "SELECT ARRAY(SELECT jsonb_array_elements_text(payload->'payload'->'mentioned_user_ids')::uuid) FROM outbox_events WHERE payload->'payload'->>'message_id' = $1",
    )
    .bind(message_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let outbox_mentions: Vec<String> = outbox_mentions.iter().map(Uuid::to_string).collect();
    assert_eq!(outbox_mentions.len(), 2);
    assert!(outbox_mentions.contains(&bobby_id));
    assert!(outbox_mentions.contains(&carol_id));
    let edit = send(
        &app,
        Call {
            method: Method::PATCH,
            uri: &format!("{message_uri}/{message_id}"),
            body: Some(json!({"content":"Now only @bobby"})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(edit.status, StatusCode::OK, "{:?}", edit.json);
    assert_eq!(
        edit.json["message"]["mentioned_user_ids"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(edit.json["message"]["mentioned_user_ids"][0], bobby_id);

    let history = get(&app, &format!("{message_uri}?limit=10"), Some(&alice)).await;
    assert_eq!(
        history.json["messages"][0]["mentioned_user_ids"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn websocket_delivery_ack_is_authenticated_scoped_and_persisted(pool: PgPool) {
    let app = app(pool);
    register(&app, "alice").await;
    register(&app, "bobby").await;
    register(&app, "carol").await;
    let alice = login_cookie(&app, "alice").await;
    let bobby = login_cookie(&app, "bobby").await;
    let carol = login_cookie(&app, "carol").await;
    let bobby_id = get(&app, "/api/v1/auth/me", Some(&bobby)).await.json["user"]["id"].clone();
    let conversation = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/conversations",
            body: Some(json!({"type":"direct", "username":"bobby"})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    let conversation_id = conversation.json["conversation"]["id"].as_str().unwrap();
    let _message = send(
        &app,
        Call {
            method: Method::POST,
            uri: &format!("/api/v1/conversations/{conversation_id}/messages"),
            body: Some(json!({"client_message_id":Uuid::now_v7(), "content":"delivered"})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server_app = app.clone();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            server_app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let url = format!("ws://{address}/api/v1/realtime");

    let mut bobby_socket = connect_websocket(&url, &bobby).await;
    bobby_socket
        .send(TungsteniteMessage::Text(
            json!({"type":"subscribe", "conversation_id":conversation_id})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    assert_eq!(
        next_websocket_json(&mut bobby_socket).await["type"],
        "subscribed"
    );
    let initial_presence = next_websocket_json(&mut bobby_socket).await;
    assert_eq!(initial_presence["type"], "presence.updated");
    assert_eq!(initial_presence["payload"]["online"], false);

    let community = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/communities",
            body: Some(json!({"name":"Realtime club", "description":""})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    let community_id = community.json["community"]["id"].as_str().unwrap();
    let invite = send(
        &app,
        Call {
            method: Method::POST,
            uri: &format!("/api/v1/communities/{community_id}/invites"),
            body: Some(json!({"max_uses":1, "ttl_seconds":600})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    let joined = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/communities/join",
            body: Some(json!({"invite_code":invite.json["invite_code"]})),
            cookie: Some(&bobby),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(joined.status, StatusCode::OK);
    let member_update = next_websocket_json(&mut bobby_socket).await;
    assert_eq!(member_update["type"], "members.updated");
    assert_eq!(member_update["payload"]["community_id"], community_id);
    let channel_created = send(
        &app,
        Call {
            method: Method::POST,
            uri: &format!("/api/v1/communities/{community_id}/channels"),
            body: Some(json!({"name":"chat", "description":""})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(channel_created.status, StatusCode::CREATED);
    let channel_update = next_websocket_json(&mut bobby_socket).await;
    assert_eq!(channel_update["type"], "channels.updated");
    assert_eq!(channel_update["payload"]["community_id"], community_id);

    let mut carol_socket = connect_websocket(&url, &carol).await;
    carol_socket
        .send(TungsteniteMessage::Text(
            json!({"type":"subscribe", "conversation_id":conversation_id})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    let denied = next_websocket_json(&mut carol_socket).await;
    assert_eq!(denied["type"], "error");
    assert_eq!(denied["code"], "NOT_FOUND");
    let channel_id = channel_created.json["channel"]["id"].as_str().unwrap();
    let renamed_channel = send(
        &app,
        Call {
            method: Method::PATCH,
            uri: &format!("/api/v1/communities/{community_id}/channels/{channel_id}"),
            body: Some(
                json!({"name":"chat-renamed", "description":"", "read_only":false, "position":0}),
            ),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(renamed_channel.status, StatusCode::OK);
    assert_eq!(
        next_websocket_json(&mut bobby_socket).await["type"],
        "channels.updated"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(150), carol_socket.next())
            .await
            .is_err()
    );

    let hidden = send(&app, Call {
        method: Method::PATCH,
        uri: "/api/v1/auth/privacy",
        body: Some(json!({"discoverable":true,"profile_visibility":"everyone","presence_visible":false})),
        cookie: Some(&alice),
        origin: Some(ORIGIN),
    }).await;
    assert_eq!(hidden.status, StatusCode::OK);
    let hidden_presence = next_websocket_json(&mut bobby_socket).await;
    assert_eq!(hidden_presence["type"], "presence.updated");
    assert_eq!(hidden_presence["payload"]["online"], Value::Null);

    let mut alice_socket_first = connect_websocket(&url, &alice).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(150), bobby_socket.next())
            .await
            .is_err()
    );
    let visible = send(&app, Call {
        method: Method::PATCH,
        uri: "/api/v1/auth/privacy",
        body: Some(json!({"discoverable":true,"profile_visibility":"everyone","presence_visible":true})),
        cookie: Some(&alice),
        origin: Some(ORIGIN),
    }).await;
    assert_eq!(visible.status, StatusCode::OK);
    let online_presence = next_websocket_json(&mut bobby_socket).await;
    assert_eq!(online_presence["payload"]["online"], true);
    assert!(
        tokio::time::timeout(Duration::from_millis(150), carol_socket.next())
            .await
            .is_err()
    );

    let mut alice_socket_second = connect_websocket(&url, &alice).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(150), bobby_socket.next())
            .await
            .is_err()
    );
    alice_socket_first.close(None).await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(150), bobby_socket.next())
            .await
            .is_err()
    );
    alice_socket_second.close(None).await.unwrap();
    let offline_presence = next_websocket_json(&mut bobby_socket).await;
    assert_eq!(offline_presence["payload"]["online"], false);

    bobby_socket
        .send(TungsteniteMessage::Text(
            json!({"type":"ack", "conversation_id":conversation_id, "sequence":1})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    let ack = next_websocket_json(&mut bobby_socket).await;
    assert_eq!(ack["type"], "delivery_ack");
    assert_eq!(ack["sequence"], 1);

    let cursors_uri = format!("/api/v1/conversations/{conversation_id}/delivery-cursors");
    let visible = get(&app, &cursors_uri, Some(&alice)).await;
    assert!(
        visible.json["cursors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|cursor| { cursor["user_id"] == bobby_id && cursor["sequence"] == 1 })
    );

    bobby_socket
        .send(TungsteniteMessage::Text(
            json!({"type":"ack", "conversation_id":conversation_id, "sequence":2})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    let invalid = next_websocket_json(&mut bobby_socket).await;
    assert_eq!(invalid["type"], "error");
    assert_eq!(invalid["code"], "INVALID_DELIVERY_CURSOR");

    bobby_socket.close(None).await.unwrap();
    carol_socket.close(None).await.unwrap();
    server.abort();
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn websocket_closes_and_allows_reconnect_after_read_cursor_bus_lag(pool: PgPool) {
    let (read_cursor_sender, _) = tokio::sync::broadcast::channel(1);
    let events = AppEvents {
        messages: tokio::sync::broadcast::channel(8).0,
        typing: tokio::sync::broadcast::channel(8).0,
        read_cursors: read_cursor_sender.clone(),
        reactions: tokio::sync::broadcast::channel(8).0,
        delivery_cursors: tokio::sync::broadcast::channel(8).0,
        presence: tokio::sync::broadcast::channel(8).0,
        community: tokio::sync::broadcast::channel(8).0,
    };
    let app = app_with_readiness_probe(
        pool,
        RegistrationPolicy::Open,
        100,
        None,
        Some(events),
        HashSet::new(),
        || async { true },
    );
    register(&app, "alice").await;
    register(&app, "bobby").await;
    let alice = login_cookie(&app, "alice").await;
    let bobby = login_cookie(&app, "bobby").await;
    let created = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/conversations",
            body: Some(json!({"type":"direct", "username":"bobby"})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED);
    let conversation_id = created.json["conversation"]["id"].as_str().unwrap();
    let alice_id = get(&app, "/api/v1/auth/me", Some(&alice)).await.json["user"]["id"]
        .as_str()
        .unwrap()
        .parse::<Uuid>()
        .unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server_app = app.clone();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            server_app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let url = format!("ws://{address}/api/v1/realtime");
    let mut socket = connect_websocket(&url, &bobby).await;
    socket
        .send(TungsteniteMessage::Text(
            json!({"type":"subscribe", "conversation_id":conversation_id})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    assert_eq!(next_websocket_json(&mut socket).await["type"], "subscribed");
    assert_eq!(
        next_websocket_json(&mut socket).await["type"],
        "presence.updated"
    );

    for sequence in 1..=1024 {
        read_cursor_sender
            .send(ReadCursorEvent {
                conversation_id: Uuid::parse_str(conversation_id).unwrap(),
                user_id: alice_id,
                sequence,
                occurred_at: time::OffsetDateTime::now_utc(),
            })
            .unwrap();
    }
    let close = tokio::time::timeout(Duration::from_secs(3), socket.next())
        .await
        .expect("lagged socket should close")
        .expect("websocket stream should yield a frame")
        .expect("websocket frame should be valid");
    let TungsteniteMessage::Close(Some(frame)) = close else {
        panic!("expected a retryable websocket close, received {close:?}");
    };
    assert_eq!(u16::from(frame.code), 1013);

    let mut reconnected = connect_websocket(&url, &bobby).await;
    reconnected
        .send(TungsteniteMessage::Text(
            json!({"type":"subscribe", "conversation_id":conversation_id})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    assert_eq!(
        next_websocket_json(&mut reconnected).await["type"],
        "subscribed"
    );
    reconnected.close(None).await.unwrap();
    server.abort();
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn websocket_closes_and_allows_reconnect_after_community_bus_lag(pool: PgPool) {
    let (community_sender, _) = tokio::sync::broadcast::channel(1);
    let events = AppEvents {
        messages: tokio::sync::broadcast::channel(8).0,
        typing: tokio::sync::broadcast::channel(8).0,
        read_cursors: tokio::sync::broadcast::channel(8).0,
        reactions: tokio::sync::broadcast::channel(8).0,
        delivery_cursors: tokio::sync::broadcast::channel(8).0,
        presence: tokio::sync::broadcast::channel(8).0,
        community: community_sender.clone(),
    };
    let app = app_with_readiness_probe(
        pool,
        RegistrationPolicy::Open,
        100,
        None,
        Some(events),
        HashSet::new(),
        || async { true },
    );
    register(&app, "alice").await;
    let alice = login_cookie(&app, "alice").await;
    let created = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/communities",
            body: Some(json!({"name":"Lag test", "description":""})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED);
    let community_id = Uuid::parse_str(created.json["community"]["id"].as_str().unwrap()).unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server_app = app.clone();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            server_app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let url = format!("ws://{address}/api/v1/realtime");
    let mut socket = connect_websocket(&url, &alice).await;

    for _ in 0..1024 {
        community_sender
            .send(CommunityEvent {
                community_id,
                kind: "channels.updated",
                occurred_at: time::OffsetDateTime::now_utc(),
            })
            .unwrap();
    }
    let close = tokio::time::timeout(Duration::from_secs(3), socket.next())
        .await
        .expect("lagged community socket should close")
        .expect("websocket stream should yield a frame")
        .expect("websocket frame should be valid");
    let TungsteniteMessage::Close(Some(frame)) = close else {
        panic!("expected a retryable websocket close, received {close:?}");
    };
    assert_eq!(u16::from(frame.code), 1013);

    let mut reconnected = connect_websocket(&url, &alice).await;
    reconnected
        .send(TungsteniteMessage::Ping(vec![1].into()))
        .await
        .unwrap();
    let pong = tokio::time::timeout(Duration::from_secs(3), reconnected.next())
        .await
        .expect("reconnected socket should respond")
        .expect("websocket stream should yield a frame")
        .expect("websocket frame should be valid");
    assert!(matches!(pong, TungsteniteMessage::Pong(payload) if payload.as_ref() == [1]));
    reconnected.close(None).await.unwrap();
    server.abort();
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn websocket_closes_and_allows_resubscribe_after_message_bus_lag(pool: PgPool) {
    let (message_sender, _) = tokio::sync::broadcast::channel(1);
    let events = AppEvents {
        messages: message_sender.clone(),
        typing: tokio::sync::broadcast::channel(8).0,
        read_cursors: tokio::sync::broadcast::channel(8).0,
        reactions: tokio::sync::broadcast::channel(8).0,
        delivery_cursors: tokio::sync::broadcast::channel(8).0,
        presence: tokio::sync::broadcast::channel(8).0,
        community: tokio::sync::broadcast::channel(8).0,
    };
    let app = app_with_readiness_probe(
        pool,
        RegistrationPolicy::Open,
        100,
        None,
        Some(events),
        HashSet::new(),
        || async { true },
    );
    register(&app, "alice").await;
    register(&app, "bobby").await;
    let alice = login_cookie(&app, "alice").await;
    let bobby = login_cookie(&app, "bobby").await;
    let created = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/conversations",
            body: Some(json!({"type":"direct", "username":"bobby"})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED);
    let conversation_id =
        Uuid::parse_str(created.json["conversation"]["id"].as_str().unwrap()).unwrap();
    let bobby_id = Uuid::parse_str(
        get(&app, "/api/v1/auth/me", Some(&bobby)).await.json["user"]["id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server_app = app.clone();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            server_app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let url = format!("ws://{address}/api/v1/realtime");
    let mut socket = connect_websocket(&url, &bobby).await;
    let subscribe = json!({"type":"subscribe", "conversation_id":conversation_id});
    socket
        .send(TungsteniteMessage::Text(subscribe.to_string().into()))
        .await
        .unwrap();
    assert_eq!(next_websocket_json(&mut socket).await["type"], "subscribed");

    for sequence in 1..=1024 {
        message_sender
            .send(Message {
                id: MessageId::from_uuid(Uuid::now_v7()),
                conversation_id: ConversationId::from_uuid(conversation_id),
                sender_id: UserId::from_uuid(bobby_id),
                sequence,
                client_message_id: Uuid::now_v7(),
                reply_to_message_id: None,
                mentioned_user_ids: Vec::new(),
                content: MessageContent::parse("lag test".to_owned()).unwrap(),
                created_at: time::OffsetDateTime::now_utc(),
                edited_at: None,
                deleted_at: None,
            })
            .unwrap();
    }
    let frame = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let frame = socket
                .next()
                .await
                .expect("websocket stream should yield a frame")
                .expect("websocket frame should be valid");
            if let TungsteniteMessage::Close(Some(frame)) = frame {
                break frame;
            }
        }
    })
    .await
    .expect("lagged message socket should close");
    assert_eq!(u16::from(frame.code), 1013);

    let mut reconnected = connect_websocket(&url, &bobby).await;
    reconnected
        .send(TungsteniteMessage::Text(subscribe.to_string().into()))
        .await
        .unwrap();
    assert_eq!(
        next_websocket_json(&mut reconnected).await["type"],
        "subscribed"
    );
    reconnected.close(None).await.unwrap();
    server.abort();
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn websocket_disconnects_a_real_slow_reader_after_send_timeout(pool: PgPool) {
    let (message_sender, _) = tokio::sync::broadcast::channel(16_384);
    let events = AppEvents {
        messages: message_sender.clone(),
        typing: tokio::sync::broadcast::channel(8).0,
        read_cursors: tokio::sync::broadcast::channel(8).0,
        reactions: tokio::sync::broadcast::channel(8).0,
        delivery_cursors: tokio::sync::broadcast::channel(8).0,
        presence: tokio::sync::broadcast::channel(8).0,
        community: tokio::sync::broadcast::channel(8).0,
    };
    let app = app_with_readiness_probe(
        pool,
        RegistrationPolicy::Open,
        100,
        None,
        Some(events),
        HashSet::new(),
        || async { true },
    );
    register(&app, "alice").await;
    register(&app, "bobby").await;
    let alice = login_cookie(&app, "alice").await;
    let bobby = login_cookie(&app, "bobby").await;
    let created = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/conversations",
            body: Some(json!({"type":"direct", "username":"bobby"})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    let conversation_id =
        Uuid::parse_str(created.json["conversation"]["id"].as_str().unwrap()).unwrap();
    let bobby_id = Uuid::parse_str(
        get(&app, "/api/v1/auth/me", Some(&bobby)).await.json["user"]["id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server_app = app.clone();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            server_app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let url = format!("ws://{address}/api/v1/realtime");
    let mut socket = connect_websocket(&url, &bobby).await;
    socket
        .send(TungsteniteMessage::Text(
            json!({"type":"subscribe", "conversation_id":conversation_id})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    assert_eq!(next_websocket_json(&mut socket).await["type"], "subscribed");

    // Never read from the client socket. Large valid message frames force real
    // TCP backpressure, exercising the gateway's write timeout rather than a mock sink.
    let content = "x".repeat(4_000);
    for sequence in 1..=8_192 {
        message_sender
            .send(Message {
                id: MessageId::from_uuid(Uuid::now_v7()),
                conversation_id: ConversationId::from_uuid(conversation_id),
                sender_id: UserId::from_uuid(bobby_id),
                sequence,
                client_message_id: Uuid::now_v7(),
                reply_to_message_id: None,
                mentioned_user_ids: Vec::new(),
                content: MessageContent::parse(content.clone()).unwrap(),
                created_at: time::OffsetDateTime::now_utc(),
                edited_at: None,
                deleted_at: None,
            })
            .unwrap();
    }
    tokio::time::sleep(Duration::from_secs(6)).await;
    let close_or_disconnect = tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            match socket.next().await {
                Some(Ok(TungsteniteMessage::Close(_))) | None | Some(Err(_)) => break,
                Some(Ok(_)) => {}
            }
        }
    })
    .await;
    assert!(
        close_or_disconnect.is_ok(),
        "stalled WebSocket reader must be disconnected within the bounded send timeout"
    );
    server.abort();
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn websocket_closes_and_allows_resubscribe_after_reaction_and_delivery_bus_lag(pool: PgPool) {
    let (reaction_sender, _) = tokio::sync::broadcast::channel(1);
    let (delivery_sender, _) = tokio::sync::broadcast::channel(1);
    let events = AppEvents {
        messages: tokio::sync::broadcast::channel(8).0,
        typing: tokio::sync::broadcast::channel(8).0,
        read_cursors: tokio::sync::broadcast::channel(8).0,
        reactions: reaction_sender.clone(),
        delivery_cursors: delivery_sender.clone(),
        presence: tokio::sync::broadcast::channel(8).0,
        community: tokio::sync::broadcast::channel(8).0,
    };
    let app = app_with_readiness_probe(
        pool,
        RegistrationPolicy::Open,
        100,
        None,
        Some(events),
        HashSet::new(),
        || async { true },
    );
    register(&app, "alice").await;
    register(&app, "bobby").await;
    let alice = login_cookie(&app, "alice").await;
    let bobby = login_cookie(&app, "bobby").await;
    let created = send(
        &app,
        Call {
            method: Method::POST,
            uri: "/api/v1/conversations",
            body: Some(json!({"type":"direct", "username":"bobby"})),
            cookie: Some(&alice),
            origin: Some(ORIGIN),
        },
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED);
    let conversation_id =
        Uuid::parse_str(created.json["conversation"]["id"].as_str().unwrap()).unwrap();
    let alice_id = Uuid::parse_str(
        get(&app, "/api/v1/auth/me", Some(&alice)).await.json["user"]["id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server_app = app.clone();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            server_app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let url = format!("ws://{address}/api/v1/realtime");
    let subscribe = json!({"type":"subscribe", "conversation_id":conversation_id});
    let mut socket = connect_websocket(&url, &bobby).await;
    socket
        .send(TungsteniteMessage::Text(subscribe.to_string().into()))
        .await
        .unwrap();
    assert_eq!(next_websocket_json(&mut socket).await["type"], "subscribed");

    for sequence in 1..=1024 {
        reaction_sender
            .send(ReactionEvent {
                conversation_id,
                message_id: Uuid::now_v7(),
                user_id: alice_id,
                sequence,
                emoji: "👍".to_owned(),
                added: true,
                occurred_at: time::OffsetDateTime::now_utc(),
            })
            .unwrap();
    }
    let frame = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let frame = socket
                .next()
                .await
                .expect("websocket stream should yield a frame")
                .expect("websocket frame should be valid");
            if let TungsteniteMessage::Close(Some(frame)) = frame {
                break frame;
            }
        }
    })
    .await
    .expect("lagged reaction socket should close");
    assert_eq!(u16::from(frame.code), 1013);

    let mut socket = connect_websocket(&url, &bobby).await;
    socket
        .send(TungsteniteMessage::Text(subscribe.to_string().into()))
        .await
        .unwrap();
    assert_eq!(next_websocket_json(&mut socket).await["type"], "subscribed");
    for sequence in 1..=1024 {
        delivery_sender
            .send(DeliveryCursorEvent {
                conversation_id,
                user_id: alice_id,
                sequence,
                occurred_at: time::OffsetDateTime::now_utc(),
            })
            .unwrap();
    }
    let frame = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let frame = socket
                .next()
                .await
                .expect("websocket stream should yield a frame")
                .expect("websocket frame should be valid");
            if let TungsteniteMessage::Close(Some(frame)) = frame {
                break frame;
            }
        }
    })
    .await
    .expect("lagged delivery-cursor socket should close");
    assert_eq!(u16::from(frame.code), 1013);

    let mut reconnected = connect_websocket(&url, &bobby).await;
    reconnected
        .send(TungsteniteMessage::Text(subscribe.to_string().into()))
        .await
        .unwrap();
    assert_eq!(
        next_websocket_json(&mut reconnected).await["type"],
        "subscribed"
    );
    reconnected.close(None).await.unwrap();
    server.abort();
}

async fn send_raw(app: &Router, uri: &str, body: &str, content_type: &str) -> Reply {
    let mut request = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::ORIGIN, ORIGIN)
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(body.to_owned()))
        .unwrap();
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
