//! General API rate limits through the real router and PostgreSQL.
//! Requires `DATABASE_URL` (see README "Local services").

mod common;

use axum::http::{Method, StatusCode, header};
use common::{Options, Reply, app, call, register, sign_in};
use rustchat_application::{ports::RateLimitRule, ratelimit::ApiRateLimitConfig};
use rustchat_infrastructure::MIGRATOR;
use serde_json::json;
use sqlx::PgPool;
use time::Duration;

fn limits(read: u32, write: u32, message: u32, create: u32, unauthenticated: u32) -> Options {
    let rule = |max| RateLimitRule {
        max,
        window: Duration::minutes(1),
    };
    Options {
        api_limits: Some(ApiRateLimitConfig {
            reads_per_user: rule(read),
            writes_per_user: rule(write),
            messages_per_user: rule(message),
            creations_per_user: rule(create),
            unauthenticated_per_ip: rule(unauthenticated),
        }),
        ..Options::default()
    }
}

fn assert_limited(reply: &Reply) {
    assert_eq!(
        reply.status,
        StatusCode::TOO_MANY_REQUESTS,
        "{:?}",
        reply.json
    );
    assert_eq!(reply.code(), "RATE_LIMITED");
    let wait: i64 = reply
        .headers
        .get(header::RETRY_AFTER)
        .expect("Retry-After")
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!((1..=60).contains(&wait), "Retry-After {wait}");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn reads_are_limited_per_user_without_affecting_other_users(pool: PgPool) {
    let app = app(pool, limits(3, 100, 100, 100, 100));
    let alice = sign_in(&app, "alice").await;
    let bobby = sign_in(&app, "bobby").await;

    for _ in 0..3 {
        let me = call(&app, Method::GET, "/api/v1/auth/me", None, Some(&alice)).await;
        assert_eq!(me.status, StatusCode::OK);
    }
    assert_limited(&call(&app, Method::GET, "/api/v1/auth/me", None, Some(&alice)).await);
    // A different user has their own budget.
    let other = call(&app, Method::GET, "/api/v1/auth/me", None, Some(&bobby)).await;
    assert_eq!(other.status, StatusCode::OK);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn message_sends_and_creations_have_their_own_budgets(pool: PgPool) {
    let app = app(pool, limits(1000, 1000, 2, 2, 100));
    let alice = sign_in(&app, "alice").await;
    assert_eq!(register(&app, "bobby").await.status, StatusCode::CREATED);

    let conversation = call(
        &app,
        Method::POST,
        "/api/v1/conversations",
        Some(json!({"type":"direct","username":"bobby"})),
        Some(&alice),
    )
    .await;
    assert_eq!(conversation.status, StatusCode::CREATED);
    let id = conversation.json["conversation"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let messages = format!("/api/v1/conversations/{id}/messages");

    // Two sends fit the message budget; the third is limited.
    for n in 0..2 {
        let sent = call(
            &app,
            Method::POST,
            &messages,
            Some(json!({"client_message_id": format!("0190f2a0-0000-7000-8000-00000000000{n}"), "content": "hi"})),
            Some(&alice),
        )
        .await;
        assert_eq!(sent.status, StatusCode::CREATED, "{:?}", sent.json);
    }
    assert_limited(
        &call(
            &app,
            Method::POST,
            &messages,
            Some(json!({"client_message_id": "0190f2a0-0000-7000-8000-000000000009", "content": "hi"})),
            Some(&alice),
        )
        .await,
    );

    // Reads and the creation budget are untouched by the message flood
    // (the first creation above used one of two).
    let list = call(
        &app,
        Method::GET,
        "/api/v1/conversations",
        None,
        Some(&alice),
    )
    .await;
    assert_eq!(list.status, StatusCode::OK);
    let community = call(
        &app,
        Method::POST,
        "/api/v1/communities",
        Some(json!({"name": "Study Room"})),
        Some(&alice),
    )
    .await;
    assert_eq!(community.status, StatusCode::CREATED);
    assert_limited(
        &call(
            &app,
            Method::POST,
            "/api/v1/communities",
            Some(json!({"name": "Another"})),
            Some(&alice),
        )
        .await,
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn other_writes_use_the_general_write_budget(pool: PgPool) {
    let app = app(pool, limits(1000, 2, 100, 100, 100));
    let alice = sign_in(&app, "alice").await;
    let profile = |name: &str| Some(json!({"display_name": name, "bio": ""}));

    for name in ["A", "B"] {
        let saved = call(
            &app,
            Method::PATCH,
            "/api/v1/auth/profile",
            profile(name),
            Some(&alice),
        )
        .await;
        assert_eq!(saved.status, StatusCode::OK);
    }
    assert_limited(
        &call(
            &app,
            Method::PATCH,
            "/api/v1/auth/profile",
            profile("C"),
            Some(&alice),
        )
        .await,
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn failed_authentication_is_limited_per_network(pool: PgPool) {
    let app = app(pool, limits(1000, 1000, 100, 100, 3));

    for _ in 0..3 {
        let anonymous = call(&app, Method::GET, "/api/v1/auth/me", None, None).await;
        assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
    }
    // The next unauthenticated request is cut off, with or without a (bad) cookie.
    assert_limited(&call(&app, Method::GET, "/api/v1/auth/me", None, None).await);
    assert_limited(
        &call(
            &app,
            Method::GET,
            "/api/v1/auth/me",
            None,
            Some("__Host-rustchat_session=not-a-real-token"),
        )
        .await,
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn limits_are_off_unless_configured(pool: PgPool) {
    // The default test options configure no API limiter: nothing is rejected.
    let app = app(pool, Options::default());
    let alice = sign_in(&app, "alice").await;
    for _ in 0..50 {
        let me = call(&app, Method::GET, "/api/v1/auth/me", None, Some(&alice)).await;
        assert_eq!(me.status, StatusCode::OK);
    }
}
