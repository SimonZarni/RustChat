//! End-to-end API tests: real router, real PostgreSQL, real Argon2id.
//! Requires `DATABASE_URL` (see README "Local services").

use std::sync::Arc;

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use http_body_util::BodyExt;
use rustchat_application::auth::{AuthConfig, AuthService, RegistrationPolicy};
use rustchat_http::{AppState, HttpConfig, router};
use rustchat_infrastructure::{
    MIGRATOR,
    postgres::{PgSessionRepository, PgUserRepository},
    security::{Argon2Hasher, OsTokenService, SystemClock},
};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;

const ORIGIN: &str = "https://chat.example.test";
const PASSWORD: &str = "correct horse battery";
const COOKIE_NAME: &str = "__Host-rustchat_session";

fn app_with(pool: PgPool, registration: RegistrationPolicy) -> Router {
    let session_ttl = time::Duration::days(30);
    let auth = AuthService::new(
        Arc::new(PgUserRepository::new(pool.clone())),
        Arc::new(PgSessionRepository::new(pool)),
        // Low Argon2 costs keep the suite fast; the algorithm is unchanged.
        Arc::new(Argon2Hasher::with_costs(8, 1, 1).unwrap()),
        Arc::new(OsTokenService),
        Arc::new(SystemClock),
        AuthConfig {
            registration,
            session_ttl,
        },
    );
    router(AppState::new(
        Arc::new(auth),
        HttpConfig {
            allowed_origins: vec![ORIGIN.to_owned()],
            cookie_secure: true,
            session_ttl,
        },
    ))
}

fn app(pool: PgPool) -> Router {
    app_with(pool, RegistrationPolicy::Open)
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    json: Value,
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
    let response = app
        .clone()
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap();
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
    let reply = get(&app(pool), "/healthz", None).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.json, json!({"status": "ok"}));
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

async fn send_raw(app: &Router, uri: &str, body: &str, content_type: &str) -> Reply {
    let request = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::ORIGIN, ORIGIN)
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(body.to_owned()))
        .unwrap();
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
