mod common;

use axum::http::{Method, StatusCode};
use common::{Options, app, call, register, sign_in, sign_in_existing};
use rustchat_infrastructure::MIGRATOR;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

#[sqlx::test(migrator = "MIGRATOR")]
async fn friend_message_and_mention_notifications_persist_and_clear_on_read(pool: PgPool) {
    let api = app(pool, Options::default());
    let alice = sign_in(&api, "notifyalice").await;
    register(&api, "notifybob").await;
    let bob = sign_in_existing(&api, "notifybob").await;

    let request = call(
        &api,
        Method::POST,
        "/api/v1/friend-requests",
        Some(json!({"username":"notifybob"})),
        Some(&alice),
    )
    .await;
    assert_eq!(request.status, StatusCode::OK);
    let request_id = request.json["id"].as_str().unwrap();
    let friend_inbox = call(&api, Method::GET, "/api/v1/notifications", None, Some(&bob)).await;
    assert_eq!(friend_inbox.json["unread_count"], 1);
    assert_eq!(
        friend_inbox.json["notifications"][0]["kind"],
        "friend_request"
    );
    assert_eq!(
        friend_inbox.json["notifications"][0]["actor_username"],
        "notifyalice"
    );

    let accepted = call(
        &api,
        Method::POST,
        &format!("/api/v1/friend-requests/{request_id}/accept"),
        None,
        Some(&bob),
    )
    .await;
    assert_eq!(accepted.status, StatusCode::NO_CONTENT);
    let conversation = call(
        &api,
        Method::POST,
        "/api/v1/conversations",
        Some(json!({"type":"direct","username":"notifybob"})),
        Some(&alice),
    )
    .await;
    assert_eq!(conversation.status, StatusCode::CREATED);
    let conversation_id = conversation.json["conversation"]["id"].as_str().unwrap();
    let message = call(
        &api,
        Method::POST,
        &format!("/api/v1/conversations/{conversation_id}/messages"),
        Some(json!({"client_message_id":Uuid::now_v7(),"content":"Hello @notifybob"})),
        Some(&alice),
    )
    .await;
    assert_eq!(message.status, StatusCode::CREATED);

    let inbox = call(
        &api,
        Method::GET,
        "/api/v1/notifications?limit=20",
        None,
        Some(&bob),
    )
    .await;
    assert_eq!(inbox.json["unread_count"], 2);
    let kinds = inbox.json["notifications"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["kind"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(kinds.contains(&"direct_message"));
    assert!(kinds.contains(&"mention"));
    assert!(
        inbox.json["notifications"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item.get("content").is_none())
    );

    let read_cursor = call(
        &api,
        Method::PUT,
        &format!("/api/v1/conversations/{conversation_id}/read-cursor"),
        Some(json!({"sequence":1})),
        Some(&bob),
    )
    .await;
    assert_eq!(read_cursor.status, StatusCode::OK);
    let cleared = call(&api, Method::GET, "/api/v1/notifications", None, Some(&bob)).await;
    assert_eq!(cleared.json["unread_count"], 0);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn notification_read_state_is_private_to_the_recipient(pool: PgPool) {
    let api = app(pool, Options::default());
    let alice = sign_in(&api, "privatealice").await;
    register(&api, "privatebob").await;
    let bob = sign_in_existing(&api, "privatebob").await;
    let request = call(
        &api,
        Method::POST,
        "/api/v1/friend-requests",
        Some(json!({"username":"privatebob"})),
        Some(&alice),
    )
    .await;
    let bob_inbox = call(&api, Method::GET, "/api/v1/notifications", None, Some(&bob)).await;
    let id = bob_inbox.json["notifications"][0]["id"].as_str().unwrap();
    let forbidden = call(
        &api,
        Method::PUT,
        &format!("/api/v1/notifications/{id}/read"),
        None,
        Some(&alice),
    )
    .await;
    assert_eq!(forbidden.status, StatusCode::NOT_FOUND);
    assert_eq!(request.status, StatusCode::OK);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn conversation_preferences_mute_and_filter_notifications(pool: PgPool) {
    let api = app(pool, Options::default());
    let alice = sign_in(&api, "prefalice").await;
    register(&api, "prefbob").await;
    let bob = sign_in_existing(&api, "prefbob").await;
    let created = call(
        &api,
        Method::POST,
        "/api/v1/conversations",
        Some(json!({"type":"direct","username":"prefbob"})),
        Some(&alice),
    )
    .await;
    let conversation_id = created.json["conversation"]["id"].as_str().unwrap();
    let settings_url = format!("/api/v1/conversations/{conversation_id}/notification-preferences");
    let default = call(&api, Method::GET, &settings_url, None, Some(&bob)).await;
    assert_eq!(default.status, StatusCode::OK);
    assert_eq!(default.json["messages_enabled"], true);
    let saved = call(&api, Method::PUT, &settings_url, Some(json!({"messages_enabled":true,"mentions_enabled":true,"muted":true,"muted_until":null})), Some(&bob)).await;
    assert_eq!(saved.status, StatusCode::OK);
    let message = call(
        &api,
        Method::POST,
        &format!("/api/v1/conversations/{conversation_id}/messages"),
        Some(json!({"client_message_id":Uuid::now_v7(),"content":"Muted hello"})),
        Some(&alice),
    )
    .await;
    assert_eq!(message.status, StatusCode::CREATED);
    let inbox = call(&api, Method::GET, "/api/v1/notifications", None, Some(&bob)).await;
    assert_eq!(inbox.json["unread_count"], 0);
    let invalid = call(&api, Method::PUT, &settings_url, Some(json!({"messages_enabled":true,"mentions_enabled":true,"muted":true,"muted_until":"2026-11-17T10:00:00Z"})), Some(&bob)).await;
    assert_eq!(invalid.status, StatusCode::BAD_REQUEST);
    register(&api, "prefstranger").await;
    let stranger = sign_in_existing(&api, "prefstranger").await;
    let hidden = call(&api, Method::GET, &settings_url, None, Some(&stranger)).await;
    assert_eq!(hidden.status, StatusCode::NOT_FOUND);
}
