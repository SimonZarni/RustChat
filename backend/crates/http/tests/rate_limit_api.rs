//! Rate limiting through the real router, limiter and PostgreSQL.
//! Requires `DATABASE_URL` (see README "Local services").

use std::{net::SocketAddr, num::NonZeroUsize, sync::Arc};

use axum::{
    Router,
    body::Body,
    extract::ConnectInfo,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use http_body_util::BodyExt;
use rustchat_application::{
    auth::{AuthConfig, AuthService, RegistrationPolicy},
    ports::RateLimitRule,
    ratelimit::RateLimitConfig,
};
use rustchat_http::{AppState, ClientIpSource, HttpConfig, router};
use rustchat_infrastructure::{
    MIGRATOR,
    postgres::{PgSessionRepository, PgUserRepository},
    ratelimit::InMemoryRateLimiter,
    security::{Argon2Hasher, OsTokenService, SystemClock},
};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;

const ORIGIN: &str = "https://chat.example.test";
const PASSWORD: &str = "correct horse battery";
const WRONG: &str = "wrong password!!";
const PEER_A: &str = "203.0.113.10:5000";
const PEER_B: &str = "198.51.100.20:5000";
const WINDOW_SECS: i64 = 15 * 60;

fn rule(max: u32) -> RateLimitRule {
    RateLimitRule {
        max,
        window: time::Duration::seconds(WINDOW_SECS),
    }
}

fn limits(login_ip: u32, login_user: u32, register_ip: u32) -> RateLimitConfig {
    RateLimitConfig {
        login_per_ip: rule(login_ip),
        login_failures_per_user: rule(login_user),
        register_per_ip: rule(register_ip),
    }
}

fn app(pool: PgPool, rate_limits: RateLimitConfig, source: ClientIpSource) -> Router {
    let session_ttl = time::Duration::days(30);
    let auth = AuthService::new(
        Arc::new(PgUserRepository::new(pool.clone())),
        Arc::new(PgSessionRepository::new(pool)),
        Arc::new(Argon2Hasher::with_costs(8, 1, 1).unwrap()),
        Arc::new(OsTokenService),
        Arc::new(SystemClock),
        Arc::new(InMemoryRateLimiter::new()),
        AuthConfig {
            registration: RegistrationPolicy::Open,
            session_ttl,
            rate_limits,
        },
    );
    router(AppState::new(
        Arc::new(auth),
        HttpConfig {
            allowed_origins: vec![ORIGIN.to_owned()],
            cookie_secure: true,
            session_ttl,
            client_ip: source,
        },
    ))
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    json: Value,
}

impl Reply {
    fn code(&self) -> &str {
        self.json["error"]["code"].as_str().unwrap_or("<none>")
    }

    fn retry_after(&self) -> Option<i64> {
        self.headers
            .get(header::RETRY_AFTER)?
            .to_str()
            .ok()?
            .parse()
            .ok()
    }
}

/// POST JSON from `peer`, optionally with an `X-Forwarded-For` header.
/// A `None` peer sends the request without connection info.
async fn post(
    app: &Router,
    uri: &str,
    body: Value,
    peer: Option<&str>,
    forwarded_for: Option<&str>,
) -> Reply {
    let mut builder = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::ORIGIN, ORIGIN)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(value) = forwarded_for {
        builder = builder.header("x-forwarded-for", value);
    }
    let mut request = builder.body(Body::from(body.to_string())).unwrap();
    if let Some(peer) = peer {
        request
            .extensions_mut()
            .insert(ConnectInfo(peer.parse::<SocketAddr>().unwrap()));
    }
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

async fn login(app: &Router, user: &str, password: &str, peer: &str) -> Reply {
    post(
        app,
        "/api/v1/auth/login",
        json!({"username": user, "password": password}),
        Some(peer),
        None,
    )
    .await
}

async fn register(app: &Router, user: &str, peer: &str) -> Reply {
    post(
        app,
        "/api/v1/auth/register",
        json!({"username": user, "password": PASSWORD}),
        Some(peer),
        None,
    )
    .await
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn repeated_failures_lock_the_username_out_with_a_retry_hint(pool: PgPool) {
    let app = app(pool, limits(100, 3, 100), ClientIpSource::Peer);
    assert_eq!(
        register(&app, "alice", PEER_A).await.status,
        StatusCode::CREATED
    );

    for _ in 0..3 {
        assert_eq!(
            login(&app, "alice", WRONG, PEER_A).await.status,
            StatusCode::UNAUTHORIZED
        );
    }

    // The correct password no longer helps, from this or any other network.
    for peer in [PEER_A, PEER_B] {
        let blocked = login(&app, "alice", PASSWORD, peer).await;
        assert_eq!(blocked.status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(blocked.code(), "RATE_LIMITED");
        assert!(
            blocked.headers.get(header::SET_COOKIE).is_none(),
            "a blocked login must not issue a session"
        );
        let wait = blocked.retry_after().expect("numeric Retry-After header");
        assert!((1..=WINDOW_SECS).contains(&wait), "Retry-After {wait}");
    }
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn limiting_does_not_reveal_whether_an_account_exists(pool: PgPool) {
    let app = app(pool, limits(100, 2, 100), ClientIpSource::Peer);
    register(&app, "alice", PEER_A).await;

    let mut transcripts = Vec::new();
    for name in ["alice", "ghost"] {
        let mut seen = Vec::new();
        for _ in 0..4 {
            let reply = login(&app, name, WRONG, PEER_A).await;
            seen.push((reply.status, reply.code().to_owned()));
        }
        transcripts.push(seen);
    }
    assert_eq!(transcripts[0], transcripts[1]);
    assert_eq!(transcripts[0][2].0, StatusCode::TOO_MANY_REQUESTS);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn one_network_cannot_try_many_usernames_but_others_are_unaffected(pool: PgPool) {
    let app = app(pool, limits(3, 100, 100), ClientIpSource::Peer);
    register(&app, "alice", PEER_B).await;

    for name in ["ghost1", "ghost2", "ghost3"] {
        assert_eq!(
            login(&app, name, WRONG, PEER_A).await.status,
            StatusCode::UNAUTHORIZED
        );
    }
    let blocked = login(&app, "alice", PASSWORD, PEER_A).await;
    assert_eq!(blocked.status, StatusCode::TOO_MANY_REQUESTS);

    let other = login(&app, "alice", PASSWORD, PEER_B).await;
    assert_eq!(other.status, StatusCode::OK);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn registration_is_limited_per_network(pool: PgPool) {
    let app = app(pool, limits(100, 100, 2), ClientIpSource::Peer);
    assert_eq!(
        register(&app, "alice", PEER_A).await.status,
        StatusCode::CREATED
    );
    assert_eq!(
        register(&app, "bobby", PEER_A).await.status,
        StatusCode::CREATED
    );

    let blocked = register(&app, "carol", PEER_A).await;
    assert_eq!(blocked.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(blocked.retry_after().is_some());

    assert_eq!(
        register(&app, "carol", PEER_B).await.status,
        StatusCode::CREATED
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn a_client_cannot_dodge_limits_by_spoofing_forwarding_headers(pool: PgPool) {
    // Default mode: X-Forwarded-For must be ignored entirely.
    let app = app(pool, limits(2, 100, 100), ClientIpSource::Peer);
    let mut statuses = Vec::new();
    for i in 0..4 {
        let spoofed = format!("10.9.8.{i}");
        let reply = post(
            &app,
            "/api/v1/auth/login",
            json!({"username": "ghost", "password": WRONG}),
            Some(PEER_A),
            Some(&spoofed),
        )
        .await;
        statuses.push(reply.status);
    }
    assert_eq!(
        statuses,
        [
            StatusCode::UNAUTHORIZED,
            StatusCode::UNAUTHORIZED,
            StatusCode::TOO_MANY_REQUESTS,
            StatusCode::TOO_MANY_REQUESTS
        ]
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn behind_one_trusted_proxy_clients_are_told_apart_by_the_last_entry(pool: PgPool) {
    let source = ClientIpSource::ForwardedFor {
        trusted_hops: NonZeroUsize::new(1).unwrap(),
    };
    let app = app(pool, limits(2, 100, 100), source);
    let proxy = "10.0.0.1:9000";
    let attempt = |forwarded: &'static str| {
        let app = app.clone();
        async move {
            post(
                &app,
                "/api/v1/auth/login",
                json!({"username": "ghost", "password": WRONG}),
                Some(proxy),
                Some(forwarded),
            )
            .await
            .status
        }
    };

    // Same real client (last entry); the leading entries are attacker-chosen.
    assert_eq!(
        attempt("1.1.1.1, 203.0.113.10").await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        attempt("2.2.2.2, 203.0.113.10").await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        attempt("3.3.3.3, 203.0.113.10").await,
        StatusCode::TOO_MANY_REQUESTS
    );

    // A different real client behind the same proxy has its own budget.
    assert_eq!(
        attempt("1.1.1.1, 198.51.100.20").await,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn a_request_without_connection_info_fails_closed(pool: PgPool) {
    let app = app(pool, limits(100, 100, 100), ClientIpSource::Peer);
    let reply = post(
        &app,
        "/api/v1/auth/login",
        json!({"username": "alice", "password": PASSWORD}),
        None,
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(reply.code(), "INTERNAL");
}
