mod common;

use axum::http::{Method, StatusCode};
use common::{Options, app, call, register, sign_in, sign_in_existing};
use rustchat_application::ports::NotificationRepository;
use rustchat_domain::identity::UserId;
use rustchat_infrastructure::MIGRATOR;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

#[sqlx::test(migrator = "MIGRATOR")]
async fn friend_message_and_mention_notifications_persist_and_clear_on_read(pool: PgPool) {
    let api = app(pool.clone(), Options::default());
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

#[sqlx::test(migrator = "MIGRATOR")]
async fn dnd_schedule_persists_and_validates_timezone_and_weekdays(pool: PgPool) {
    let api = app(pool.clone(), Options::default());
    let user = sign_in(&api, "dndalice").await;
    let path = "/api/v1/notifications/dnd-schedule";
    let defaults = call(&api, Method::GET, path, None, Some(&user)).await;
    assert_eq!(defaults.status, StatusCode::OK);
    assert_eq!(defaults.json["enabled"], false);
    let saved = call(&api, Method::PUT, path, Some(json!({"enabled":true,"start_time":"22:00","end_time":"07:30","timezone":"Asia/Bangkok","weekdays":[1,2,3,4,5]})), Some(&user)).await;
    assert_eq!(saved.status, StatusCode::OK);
    assert_eq!(saved.json["timezone"], "Asia/Bangkok");
    let me = call(&api, Method::GET, "/api/v1/auth/me", None, Some(&user)).await;
    let id = Uuid::parse_str(me.json["user"]["id"].as_str().unwrap()).unwrap();
    let repository = rustchat_infrastructure::postgres::PgNotificationRepository::new(pool.clone());
    repository
        .is_in_dnd_schedule(UserId::from_uuid(id))
        .await
        .unwrap();
    let invalid = call(&api, Method::PUT, path, Some(json!({"enabled":true,"start_time":"22:00","end_time":"07:30","timezone":"No/Such_Zone","weekdays":[1,2,3]})), Some(&user)).await;
    assert_eq!(invalid.status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn background_push_is_reported_unavailable_without_vapid_configuration(pool: PgPool) {
    let api = app(pool, Options::default());
    let user = sign_in(&api, "pushdisabled").await;
    let config = call(
        &api,
        Method::GET,
        "/api/v1/notifications/push-config",
        None,
        Some(&user),
    )
    .await;
    assert_eq!(config.status, StatusCode::OK);
    assert_eq!(config.json["enabled"], false);
    assert_eq!(config.json["public_key"], serde_json::Value::Null);
    let unavailable = call(&api, Method::POST, "/api/v1/notifications/push-subscriptions", Some(json!({"endpoint":"https://updates.push.services.mozilla.com/wpush/v2/abcdefgh","expirationTime":null,"keys":{"p256dh":"x","auth":"y"}})), Some(&user)).await;
    assert_eq!(unavailable.status, StatusCode::SERVICE_UNAVAILABLE);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn community_member_join_creates_owner_notification(pool: PgPool) {
    let api = app(pool, Options::default());
    let owner = sign_in(&api, "communityowner").await;
    let member = sign_in(&api, "communitymember").await;
    let created = call(
        &api,
        Method::POST,
        "/api/v1/communities",
        Some(json!({"name":"Quiet Garden"})),
        Some(&owner),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED);
    let community_id = created.json["community"]["id"].as_str().unwrap();
    let invite = call(
        &api,
        Method::POST,
        &format!("/api/v1/communities/{community_id}/invites"),
        Some(json!({"ttl_seconds":3600,"max_uses":2})),
        Some(&owner),
    )
    .await;
    assert_eq!(invite.status, StatusCode::CREATED);
    let joined = call(
        &api,
        Method::POST,
        "/api/v1/communities/join",
        Some(json!({"invite_code":invite.json["invite_code"]})),
        Some(&member),
    )
    .await;
    assert_eq!(joined.status, StatusCode::OK);
    let prefs_url = format!("/api/v1/communities/{community_id}/notification-preferences");
    let defaults = call(&api, Method::GET, &prefs_url, None, Some(&member)).await;
    assert_eq!(defaults.status, StatusCode::OK);
    assert_eq!(defaults.json["member_events_enabled"], true);
    let saved = call(
        &api,
        Method::PUT,
        &prefs_url,
        Some(json!({"member_events_enabled":false,"channel_events_enabled":false})),
        Some(&member),
    )
    .await;
    assert_eq!(saved.status, StatusCode::OK);
    let second_member = sign_in(&api, "communitysecond").await;
    let joined_second = call(
        &api,
        Method::POST,
        "/api/v1/communities/join",
        Some(json!({"invite_code":invite.json["invite_code"]})),
        Some(&second_member),
    )
    .await;
    assert_eq!(joined_second.status, StatusCode::OK);
    let inbox = call(
        &api,
        Method::GET,
        "/api/v1/notifications",
        None,
        Some(&owner),
    )
    .await;
    assert_eq!(inbox.json["unread_count"], 2);
    assert_eq!(inbox.json["notifications"][0]["kind"], "community_event");
    assert_eq!(
        inbox.json["notifications"][0]["event_type"],
        "member_joined"
    );
    assert_eq!(inbox.json["notifications"][0]["community_id"], community_id);
    let outsider = sign_in(&api, "communityoutsider").await;
    let hidden = call(&api, Method::GET, &prefs_url, None, Some(&outsider)).await;
    assert_eq!(hidden.status, StatusCode::NOT_FOUND);
    let member_inbox = call(
        &api,
        Method::GET,
        "/api/v1/notifications",
        None,
        Some(&member),
    )
    .await;
    assert_eq!(
        member_inbox.json["unread_count"], 0,
        "muted community event categories must not create inbox items"
    );
    let channel = call(
        &api,
        Method::POST,
        &format!("/api/v1/communities/{community_id}/channels"),
        Some(json!({"name":"garden-chat"})),
        Some(&owner),
    )
    .await;
    assert_eq!(channel.status, StatusCode::CREATED);
    let second_inbox = call(
        &api,
        Method::GET,
        "/api/v1/notifications",
        None,
        Some(&second_member),
    )
    .await;
    assert_eq!(
        second_inbox.json["notifications"][0]["event_type"],
        "channel_created"
    );
}
