mod common;

use axum::http::{Method, StatusCode};
use common::{Options, app, call, register, sign_in};
use rustchat_infrastructure::MIGRATOR;
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test(migrator = "MIGRATOR")]
async fn friend_requests_and_blocks_enforce_relationship_rules(pool: PgPool) {
    let api = app(pool, Options::default());
    let alice = sign_in(&api, "socialalice").await;
    register(&api, "socialbob").await;
    let sent = call(
        &api,
        Method::POST,
        "/api/v1/friend-requests",
        Some(json!({"username":"socialbob"})),
        Some(&alice),
    )
    .await;
    assert_eq!(sent.status, StatusCode::OK);
    let request_id = sent.json["id"].as_str().unwrap();
    let incoming = call(
        &api,
        Method::GET,
        "/api/v1/friend-requests?direction=incoming",
        None,
        Some(&alice),
    )
    .await;
    assert_eq!(incoming.status, StatusCode::OK);
    let bob = sign_in_existing(&api, "socialbob").await;
    let accepted = call(
        &api,
        Method::POST,
        &format!("/api/v1/friend-requests/{request_id}/accept"),
        None,
        Some(&bob),
    )
    .await;
    assert_eq!(accepted.status, StatusCode::NO_CONTENT);
    let friends = call(&api, Method::GET, "/api/v1/friends", None, Some(&alice)).await;
    assert_eq!(friends.json["users"].as_array().unwrap().len(), 1);
    let existing_direct = call(
        &api,
        Method::POST,
        "/api/v1/conversations",
        Some(json!({"type":"direct","username":"socialbob"})),
        Some(&alice),
    )
    .await;
    assert_eq!(existing_direct.status, StatusCode::CREATED);

    let blocked = call(
        &api,
        Method::POST,
        "/api/v1/blocks",
        Some(json!({"username":"socialbob"})),
        Some(&alice),
    )
    .await;
    assert_eq!(blocked.status, StatusCode::NO_CONTENT);
    let friends_after = call(&api, Method::GET, "/api/v1/friends", None, Some(&alice)).await;
    assert!(friends_after.json["users"].as_array().unwrap().is_empty());
    let profile = call(
        &api,
        Method::GET,
        "/api/v1/users/socialbob/profile",
        None,
        Some(&alice),
    )
    .await;
    assert_eq!(profile.status, StatusCode::NOT_FOUND);
    let direct = call(
        &api,
        Method::POST,
        "/api/v1/conversations",
        Some(json!({"type":"direct","username":"socialbob"})),
        Some(&alice),
    )
    .await;
    assert_eq!(direct.status, StatusCode::NOT_FOUND);
    let group = call(
        &api,
        Method::POST,
        "/api/v1/conversations",
        Some(json!({"type":"group","usernames":["socialbob"]})),
        Some(&alice),
    )
    .await;
    assert_eq!(group.status, StatusCode::NOT_FOUND);
    let existing_id = existing_direct.json["conversation"]["id"].as_str().unwrap();
    let blocked_message = call(
        &api,
        Method::POST,
        &format!("/api/v1/conversations/{existing_id}/messages"),
        Some(json!({"client_message_id":uuid::Uuid::now_v7(),"content":"after block"})),
        Some(&alice),
    )
    .await;
    assert_eq!(blocked_message.status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn only_recipient_can_accept_and_sender_can_cancel(pool: PgPool) {
    let api = app(pool, Options::default());
    let alice = sign_in(&api, "requestalice").await;
    register(&api, "requestbob").await;
    let request = call(
        &api,
        Method::POST,
        "/api/v1/friend-requests",
        Some(json!({"username":"requestbob"})),
        Some(&alice),
    )
    .await;
    let id = request.json["id"].as_str().unwrap();
    let forbidden = call(
        &api,
        Method::POST,
        &format!("/api/v1/friend-requests/{id}/accept"),
        None,
        Some(&alice),
    )
    .await;
    assert_eq!(forbidden.status, StatusCode::NOT_FOUND);
    let canceled = call(
        &api,
        Method::DELETE,
        &format!("/api/v1/friend-requests/{id}"),
        None,
        Some(&alice),
    )
    .await;
    assert_eq!(canceled.status, StatusCode::NO_CONTENT);
}

use common::sign_in_existing;
