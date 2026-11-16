//! Email verification through the real router and PostgreSQL.
//! Requires `DATABASE_URL` (see README "Local services").

mod common;

use axum::http::{Method, StatusCode};
use common::{CapturedEmail, Options, PASSWORD, app, call, sign_in};
use rustchat_infrastructure::MIGRATOR;
use serde_json::{Value, json};
use sqlx::PgPool;

fn with_email() -> (Options, CapturedEmail) {
    let email = CapturedEmail::default();
    (
        Options {
            email: Some(email.clone()),
            ..Options::default()
        },
        email,
    )
}

async fn register_with(app: &axum::Router, username: &str, email: &str) {
    let reply = call(
        app,
        Method::POST,
        "/api/v1/auth/register",
        Some(json!({"username": username, "password": PASSWORD, "email": email})),
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.json);
}

async fn login(app: &axum::Router, username: &str) -> String {
    let reply = call(
        app,
        Method::POST,
        "/api/v1/auth/login",
        Some(json!({"username": username, "password": PASSWORD})),
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);
    reply.new_session_cookie().unwrap()
}

async fn email_state(app: &axum::Router, cookie: &str) -> Value {
    let reply = call(app, Method::GET, "/api/v1/auth/email", None, Some(cookie)).await;
    assert_eq!(reply.status, StatusCode::OK);
    reply.json["email"].clone()
}

async fn verify(app: &axum::Router, token: &str) -> StatusCode {
    call(
        app,
        Method::POST,
        "/api/v1/auth/email/verify",
        Some(json!({"token": token})),
        None,
    )
    .await
    .status
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn registration_sends_a_single_use_link_that_verifies_the_address(pool: PgPool) {
    let (options, sent) = with_email();
    let app = app(pool, options);
    register_with(&app, "alice", "Alice@Example.test").await;
    let cookie = login(&app, "alice").await;

    let before = email_state(&app, &cookie).await;
    assert_eq!(before["email"], "alice@example.test");
    assert_eq!(before["verified"], false);
    assert_eq!(sent.count().await, 1);

    let token = sent.last_token_for("alice@example.test").await.unwrap();
    assert_eq!(verify(&app, &token).await, StatusCode::NO_CONTENT);
    assert_eq!(email_state(&app, &cookie).await["verified"], true);

    // A link works once, and garbage or oversized tokens are rejected the same way.
    assert_eq!(verify(&app, &token).await, StatusCode::BAD_REQUEST);
    assert_eq!(verify(&app, "not-a-token").await, StatusCode::BAD_REQUEST);
    assert_eq!(
        verify(&app, &"x".repeat(500)).await,
        StatusCode::BAD_REQUEST
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn expired_links_are_rejected(pool: PgPool) {
    let (options, sent) = with_email();
    let app = app(pool.clone(), options);
    register_with(&app, "alice", "alice@example.test").await;
    let token = sent.last_token_for("alice@example.test").await.unwrap();

    sqlx::query("UPDATE account_tokens SET expires_at = created_at + interval '1 microsecond'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(verify(&app, &token).await, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn changing_the_address_requires_the_password_and_invalidates_old_links(pool: PgPool) {
    let (options, sent) = with_email();
    let app = app(pool, options);
    register_with(&app, "alice", "old@example.test").await;
    let cookie = login(&app, "alice").await;
    let old_token = sent.last_token_for("old@example.test").await.unwrap();

    let wrong = call(
        &app,
        Method::PUT,
        "/api/v1/auth/email",
        Some(json!({"email": "new@example.test", "current_password": "wrong password!!"})),
        Some(&cookie),
    )
    .await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        email_state(&app, &cookie).await["email"],
        "old@example.test"
    );

    let changed = call(
        &app,
        Method::PUT,
        "/api/v1/auth/email",
        Some(json!({"email": "New@Example.test", "current_password": PASSWORD})),
        Some(&cookie),
    )
    .await;
    assert_eq!(changed.status, StatusCode::OK);
    assert_eq!(changed.json["email"]["email"], "new@example.test");
    assert_eq!(changed.json["email"]["verified"], false);

    // The link for the old address no longer works; the new one does.
    assert_eq!(verify(&app, &old_token).await, StatusCode::BAD_REQUEST);
    let new_token = sent.last_token_for("new@example.test").await.unwrap();
    assert_eq!(verify(&app, &new_token).await, StatusCode::NO_CONTENT);
    assert_eq!(email_state(&app, &cookie).await["verified"], true);

    // Removing the address clears verification too.
    let removed = call(
        &app,
        Method::PUT,
        "/api/v1/auth/email",
        Some(json!({"email": null, "current_password": PASSWORD})),
        Some(&cookie),
    )
    .await;
    assert_eq!(removed.status, StatusCode::OK);
    assert_eq!(removed.json["email"]["email"], Value::Null);
    assert_eq!(removed.json["email"]["verified"], false);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn registering_with_someone_elses_address_does_not_block_or_reveal_it(pool: PgPool) {
    let (options, sent) = with_email();
    let app = app(pool, options);

    // The squatter registers first with the victim's address but never verifies it.
    register_with(&app, "squatter", "victim@example.test").await;
    // The real owner registers with the same address: accepted, no "taken" signal.
    register_with(&app, "victim", "victim@example.test").await;
    let token = sent.last_token_for("victim@example.test").await.unwrap();
    assert_eq!(verify(&app, &token).await, StatusCode::NO_CONTENT);

    // Once verified by the owner, the squatter's link can no longer claim the address.
    let squatter_cookie = login(&app, "squatter").await;
    let resend = call(
        &app,
        Method::POST,
        "/api/v1/auth/email/resend",
        None,
        Some(&squatter_cookie),
    )
    .await;
    assert_eq!(resend.status, StatusCode::ACCEPTED);
    let squatter_token = sent.last_token_for("victim@example.test").await.unwrap();
    let claim = call(
        &app,
        Method::POST,
        "/api/v1/auth/email/verify",
        Some(json!({"token": squatter_token})),
        None,
    )
    .await;
    assert_eq!(claim.status, StatusCode::CONFLICT);
    assert_eq!(claim.code(), "EMAIL_ADDRESS_IN_USE");
    assert_eq!(email_state(&app, &squatter_cookie).await["verified"], false);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn resending_is_limited_and_needs_a_session_and_configured_email(pool: PgPool) {
    let (options, sent) = with_email();
    let app = app(pool.clone(), options);
    register_with(&app, "alice", "alice@example.test").await;
    let cookie = login(&app, "alice").await;

    let anonymous = call(&app, Method::POST, "/api/v1/auth/email/resend", None, None).await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);

    // Three sends per hour; the first email went out at registration (not counted).
    for _ in 0..3 {
        let reply = call(
            &app,
            Method::POST,
            "/api/v1/auth/email/resend",
            None,
            Some(&cookie),
        )
        .await;
        assert_eq!(reply.status, StatusCode::ACCEPTED);
    }
    let limited = call(
        &app,
        Method::POST,
        "/api/v1/auth/email/resend",
        None,
        Some(&cookie),
    )
    .await;
    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(sent.count().await, 4);

    // Only the newest link works.
    let newest = sent.last_token_for("alice@example.test").await.unwrap();
    let first_token = {
        let all = sent.0.lock().await;
        all[0]
            .1
            .split("token=")
            .nth(1)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .to_owned()
    };
    assert_eq!(verify(&app, &first_token).await, StatusCode::BAD_REQUEST);
    assert_eq!(verify(&app, &newest).await, StatusCode::NO_CONTENT);

    // Without SMTP configured, resending reports that email is unavailable.
    let plain = common::app(pool, Options::default());
    let cookie = sign_in(&plain, "bobby").await;
    let unavailable = call(
        &plain,
        Method::POST,
        "/api/v1/auth/email/resend",
        None,
        Some(&cookie),
    )
    .await;
    assert_eq!(unavailable.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(unavailable.code(), "EMAIL_UNAVAILABLE");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn deleting_the_account_clears_the_verified_address(pool: PgPool) {
    let (options, sent) = with_email();
    let app = app(pool.clone(), options);
    register_with(&app, "alice", "alice@example.test").await;
    let cookie = login(&app, "alice").await;
    let token = sent.last_token_for("alice@example.test").await.unwrap();
    assert_eq!(verify(&app, &token).await, StatusCode::NO_CONTENT);

    let deleted = call(
        &app,
        Method::POST,
        "/api/v1/auth/delete",
        Some(json!({"current_password": PASSWORD, "confirm": true})),
        Some(&cookie),
    )
    .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    let remaining: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM users WHERE email IS NOT NULL OR email_verified_at IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(remaining, 0);
}
