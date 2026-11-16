//! Audit log browsing through the real router and PostgreSQL.
//! Requires `DATABASE_URL` (see README "Local services").

mod common;

use axum::http::{Method, StatusCode};
use common::{Options, app, call, register, sign_in};
use rustchat_domain::identity::UserId;
use rustchat_infrastructure::MIGRATOR;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

fn user_id(reply: &common::Reply) -> Uuid {
    Uuid::parse_str(reply.json["user"]["id"].as_str().unwrap()).unwrap()
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn instance_audit_is_admin_only_newest_first_and_paged(pool: PgPool) {
    let plain = app(pool.clone(), Options::default());
    let alice = register(&plain, "alice").await;
    let bobby = register(&plain, "bobby").await;
    let (alice_id, bobby_id) = (user_id(&alice), user_id(&bobby));
    let alice_cookie = common::sign_in_existing(&plain, "alice").await;
    let bobby_cookie = common::sign_in_existing(&plain, "bobby").await;

    let admin = app(
        pool,
        Options {
            admin_user_ids: [UserId::from_uuid(alice_id)].into(),
            ..Options::default()
        },
    );
    let suspension = format!("/api/v1/admin/users/{bobby_id}/suspension");
    for suspended in [true, false, true] {
        let reply = call(
            &admin,
            Method::PUT,
            &suspension,
            Some(json!({ "suspended": suspended })),
            Some(&alice_cookie),
        )
        .await;
        assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.json);
    }

    // Only configured administrators can read it; anonymous callers cannot.
    let forbidden = call(
        &admin,
        Method::GET,
        "/api/v1/admin/audit",
        None,
        Some(&bobby_cookie),
    )
    .await;
    assert_eq!(
        forbidden.status,
        StatusCode::UNAUTHORIZED,
        "bobby was suspended, sessions revoked"
    );
    let anonymous = call(&admin, Method::GET, "/api/v1/admin/audit", None, None).await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);

    let page = call(
        &admin,
        Method::GET,
        "/api/v1/admin/audit?limit=2",
        None,
        Some(&alice_cookie),
    )
    .await;
    assert_eq!(page.status, StatusCode::OK, "{:?}", page.json);
    let events = page.json["events"].as_array().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["event_type"], "user.suspended");
    assert_eq!(events[1]["event_type"], "user.unsuspended");
    assert_eq!(events[0]["actor"]["username"], "alice");
    assert_eq!(events[0]["target"]["username"], "bobby");
    assert_eq!(events[0]["details"], json!({"suspended": true}));
    let cursor = page.json["next_cursor"]
        .as_str()
        .expect("another page exists")
        .to_owned();

    let rest = call(
        &admin,
        Method::GET,
        &format!("/api/v1/admin/audit?limit=2&before={cursor}"),
        None,
        Some(&alice_cookie),
    )
    .await;
    let remaining = rest.json["events"].as_array().unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0]["event_type"], "user.suspended");
    assert_eq!(rest.json["next_cursor"], Value::Null);
    // No overlap between pages.
    assert_ne!(remaining[0]["id"], events[1]["id"]);

    let bad = call(
        &admin,
        Method::GET,
        "/api/v1/admin/audit?before=nope",
        None,
        Some(&alice_cookie),
    )
    .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    let unknown = call(
        &admin,
        Method::GET,
        &format!("/api/v1/admin/audit?before={}", Uuid::now_v7()),
        None,
        Some(&alice_cookie),
    )
    .await;
    assert_eq!(unknown.json["events"].as_array().unwrap().len(), 0);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn a_non_admin_gets_forbidden_from_the_instance_audit(pool: PgPool) {
    let plain = app(pool, Options::default());
    let cookie = sign_in(&plain, "alice").await;
    let reply = call(
        &plain,
        Method::GET,
        "/api/v1/admin/audit",
        None,
        Some(&cookie),
    )
    .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn community_audit_is_visible_to_managers_only_and_records_their_actions(pool: PgPool) {
    let app = app(pool, Options::default());
    let owner = sign_in(&app, "owner").await;
    let member = sign_in(&app, "member").await;

    let created = call(
        &app,
        Method::POST,
        "/api/v1/communities",
        Some(json!({"name": "Study Room"})),
        Some(&owner),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.json);
    let id = created.json["community"]["id"].as_str().unwrap().to_owned();

    let renamed = call(
        &app,
        Method::PATCH,
        &format!("/api/v1/communities/{id}"),
        Some(json!({"name": "Study Hall", "description": "quiet"})),
        Some(&owner),
    )
    .await;
    assert_eq!(renamed.status, StatusCode::OK, "{:?}", renamed.json);
    let invite = call(
        &app,
        Method::POST,
        &format!("/api/v1/communities/{id}/invites"),
        Some(json!({"ttl_seconds": 3600, "max_uses": 5})),
        Some(&owner),
    )
    .await;
    assert_eq!(invite.status, StatusCode::CREATED, "{:?}", invite.json);
    let joined = call(
        &app,
        Method::POST,
        "/api/v1/communities/join",
        Some(json!({"invite_code": invite.json["invite_code"]})),
        Some(&member),
    )
    .await;
    assert_eq!(joined.status, StatusCode::OK, "{:?}", joined.json);

    let uri = format!("/api/v1/communities/{id}/audit");
    let log = call(&app, Method::GET, &uri, None, Some(&owner)).await;
    assert_eq!(log.status, StatusCode::OK, "{:?}", log.json);
    let events = log.json["events"].as_array().unwrap();
    let kinds: Vec<&str> = events
        .iter()
        .map(|e| e["event_type"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        ["invite.created", "settings.updated"],
        "newest first"
    );
    assert_eq!(events[0]["actor"]["username"], "owner");
    assert_eq!(events[1]["details"]["name"], "Study Hall");

    // An ordinary member cannot tell the log exists; neither can an outsider.
    let denied = call(&app, Method::GET, &uri, None, Some(&member)).await;
    assert_eq!(denied.status, StatusCode::NOT_FOUND);
    let outsider = sign_in(&app, "outsider").await;
    assert_eq!(
        call(&app, Method::GET, &uri, None, Some(&outsider))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, Method::GET, &uri, None, None).await.status,
        StatusCode::UNAUTHORIZED
    );

    // Another community's events never appear.
    let other = call(
        &app,
        Method::POST,
        "/api/v1/communities",
        Some(json!({"name": "Other"})),
        Some(&owner),
    )
    .await;
    let other_id = other.json["community"]["id"].as_str().unwrap();
    let other_log = call(
        &app,
        Method::GET,
        &format!("/api/v1/communities/{other_id}/audit"),
        None,
        Some(&owner),
    )
    .await;
    assert_eq!(other_log.json["events"].as_array().unwrap().len(), 0);

    // Paging.
    let first = call(
        &app,
        Method::GET,
        &format!("{uri}?limit=1"),
        None,
        Some(&owner),
    )
    .await;
    assert_eq!(first.json["events"].as_array().unwrap().len(), 1);
    let cursor = first.json["next_cursor"].as_str().unwrap();
    let second = call(
        &app,
        Method::GET,
        &format!("{uri}?limit=1&before={cursor}"),
        None,
        Some(&owner),
    )
    .await;
    assert_eq!(second.json["events"][0]["event_type"], "settings.updated");
    assert_eq!(second.json["next_cursor"], Value::Null);
}
