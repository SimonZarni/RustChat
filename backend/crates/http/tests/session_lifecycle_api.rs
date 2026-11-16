//! Session idle timeout and token rotation through the real router and PostgreSQL.
//! Requires `DATABASE_URL` (see README "Local services").

mod common;

use axum::http::{Method, StatusCode};
use common::{Options, app, call, sign_in};
use rustchat_application::auth::SessionLifecycle;
use rustchat_infrastructure::MIGRATOR;
use sqlx::PgPool;
use std::time::Duration as StdDuration;
use time::Duration;

fn lifecycle(idle: Duration, rotation: Duration, touch: Duration) -> Options {
    Options {
        lifecycle: SessionLifecycle {
            idle_timeout: idle,
            rotation_interval: rotation,
            rotation_grace: Duration::seconds(60),
            touch_interval: touch,
        },
        ..Options::default()
    }
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn an_idle_session_is_rejected_but_activity_keeps_it_alive(pool: PgPool) {
    // 2 s idle timeout, activity recorded on every request, no rotation.
    let app = app(
        pool,
        lifecycle(Duration::seconds(2), Duration::days(30), Duration::ZERO),
    );
    let cookie = sign_in(&app, "alice").await;

    // Requests inside the window keep extending it beyond the original 2 s.
    for _ in 0..3 {
        tokio::time::sleep(StdDuration::from_millis(1200)).await;
        let me = call(&app, Method::GET, "/api/v1/auth/me", None, Some(&cookie)).await;
        assert_eq!(me.status, StatusCode::OK, "active session must stay valid");
    }

    tokio::time::sleep(StdDuration::from_millis(2300)).await;
    let me = call(&app, Method::GET, "/api/v1/auth/me", None, Some(&cookie)).await;
    assert_eq!(me.status, StatusCode::UNAUTHORIZED);
    assert_eq!(me.code(), "UNAUTHENTICATED");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn the_session_token_rotates_and_the_old_one_works_only_during_the_grace_period(
    pool: PgPool,
) {
    // Rotate as soon as a request is authenticated.
    let app = app(
        pool.clone(),
        lifecycle(Duration::days(7), Duration::ZERO, Duration::minutes(5)),
    );
    let first = sign_in(&app, "alice").await;

    let rotated = call(&app, Method::GET, "/api/v1/auth/me", None, Some(&first)).await;
    assert_eq!(rotated.status, StatusCode::OK);
    let second = rotated
        .new_session_cookie()
        .expect("a rotated session cookie is set");
    assert_ne!(second, first);
    let set_cookie = rotated
        .headers
        .get_all(axum::http::header::SET_COOKIE)
        .iter()
        .next()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    for attribute in ["HttpOnly", "Secure", "SameSite=Strict", "Path=/"] {
        assert!(
            set_cookie.contains(attribute),
            "{attribute} in {set_cookie}"
        );
    }

    // A parallel request still carrying the old token is not signed out, and does not rotate again.
    let in_flight = call(&app, Method::GET, "/api/v1/auth/me", None, Some(&first)).await;
    assert_eq!(in_flight.status, StatusCode::OK);
    assert!(in_flight.new_session_cookie().is_none());

    // The new token is the live one (and, with a zero interval, rotates again).
    let with_new = call(&app, Method::GET, "/api/v1/auth/me", None, Some(&second)).await;
    assert_eq!(with_new.status, StatusCode::OK);
    let third = with_new.new_session_cookie().expect("rotates again");

    // Only digests are stored: the raw tokens never appear in the table.
    let stored: Vec<(Vec<u8>, Option<Vec<u8>>)> =
        sqlx::query_as("SELECT token_digest, previous_token_digest FROM sessions")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(stored.len(), 1);
    for token in [&first, &second, &third] {
        let raw = token.split('=').nth(1).unwrap().as_bytes();
        assert!(stored.iter().all(|(current, previous)| {
            current.as_slice() != raw && previous.as_deref() != Some(raw)
        }));
    }

    // Expire the grace period: the original token is now dead, the latest still works.
    sqlx::query("UPDATE sessions SET previous_token_valid_until = now() - interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();
    let stale = call(&app, Method::GET, "/api/v1/auth/me", None, Some(&second)).await;
    assert_eq!(stale.status, StatusCode::UNAUTHORIZED);
    let live = call(&app, Method::GET, "/api/v1/auth/me", None, Some(&third)).await;
    assert_eq!(live.status, StatusCode::OK);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn logout_does_not_resurrect_the_session_with_a_rotated_cookie(pool: PgPool) {
    let app = app(
        pool,
        lifecycle(Duration::days(7), Duration::ZERO, Duration::minutes(5)),
    );
    let cookie = sign_in(&app, "alice").await;

    let logout = call(
        &app,
        Method::POST,
        "/api/v1/auth/logout",
        None,
        Some(&cookie),
    )
    .await;
    assert_eq!(logout.status, StatusCode::NO_CONTENT);
    // The only session cookie in the response clears it; no replacement token is set.
    assert!(logout.new_session_cookie().is_none());
    let cookies = logout.session_cookies();
    assert_eq!(cookies.len(), 1, "{cookies:?}");
    assert!(
        cookies[0].ends_with('='),
        "removal cookie expected: {cookies:?}"
    );

    let me = call(&app, Method::GET, "/api/v1/auth/me", None, Some(&cookie)).await;
    assert_eq!(me.status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn activity_is_recorded_at_most_once_per_touch_interval(pool: PgPool) {
    let app = app(
        pool.clone(),
        lifecycle(Duration::days(7), Duration::days(30), Duration::minutes(5)),
    );
    let cookie = sign_in(&app, "alice").await;
    let before: time::OffsetDateTime = sqlx::query_scalar("SELECT last_seen_at FROM sessions")
        .fetch_one(&pool)
        .await
        .unwrap();

    for _ in 0..3 {
        let me = call(&app, Method::GET, "/api/v1/auth/me", None, Some(&cookie)).await;
        assert_eq!(me.status, StatusCode::OK);
    }
    let after: time::OffsetDateTime = sqlx::query_scalar("SELECT last_seen_at FROM sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(before, after, "no write inside the touch interval");
}
