mod common;

use axum::http::{Method, StatusCode};
use common::{Options, app, call, sign_in};
use rustchat_infrastructure::MIGRATOR;
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test(migrator = "MIGRATOR")]
async fn voice_tokens_are_short_lived_room_scoped_and_membership_checked(pool: PgPool) {
    let api = app(
        pool,
        Options {
            voice: Some(rustchat_http::VoiceConfig {
                url: "wss://voice.example.test".into(),
                api_key: "test-api-key".into(),
                api_secret: "test-secret-with-enough-entropy".into(),
            }),
            ..Options::default()
        },
    );
    let owner = sign_in(&api, "voiceowner").await;
    let outsider = sign_in(&api, "voiceoutsider").await;
    let created = call(
        &api,
        Method::POST,
        "/api/v1/communities",
        Some(json!({"name":"Voice Space"})),
        Some(&owner),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED);
    let community_id = created.json["community"]["id"].as_str().unwrap();
    let channel = call(
        &api,
        Method::POST,
        &format!("/api/v1/communities/{community_id}/channels"),
        Some(json!({"name":"hangout","channel_type":"voice"})),
        Some(&owner),
    )
    .await;
    assert_eq!(channel.status, StatusCode::CREATED, "{:?}", channel.json);
    let channel_id = channel.json["channel"]["id"].as_str().unwrap();

    let unauthorized = call(
        &api,
        Method::POST,
        &format!("/api/v1/channels/{channel_id}/voice-token"),
        None,
        Some(&outsider),
    )
    .await;
    assert_eq!(unauthorized.status, StatusCode::NOT_FOUND);
    let issued = call(
        &api,
        Method::POST,
        &format!("/api/v1/channels/{channel_id}/voice-token"),
        None,
        Some(&owner),
    )
    .await;
    assert_eq!(issued.status, StatusCode::OK, "{:?}", issued.json);
    assert_eq!(issued.json["url"], "wss://voice.example.test");
    let jwt = issued.json["token"].as_str().unwrap();
    let claims: serde_json::Value = serde_json::from_slice(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(jwt.split('.').nth(1).unwrap())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(claims["iss"], "test-api-key");
    assert_eq!(claims["video"]["room"], format!("rustchat-{channel_id}"));
    assert_eq!(claims["video"]["roomJoin"], true);
    assert_eq!(claims["video"]["canPublish"], true);
    assert_eq!(claims["video"]["canPublishSources"][0], "microphone");
    assert!(claims["exp"].as_i64().unwrap() - claims["nbf"].as_i64().unwrap() <= 605);

    let roles = call(
        &api,
        Method::GET,
        &format!("/api/v1/communities/{community_id}/roles"),
        None,
        Some(&owner),
    )
    .await;
    let default_role = roles.json["roles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|role| role["is_default"] == true)
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let denied = call(
        &api,
        Method::PUT,
        &format!("/api/v1/channels/{channel_id}/permissions/{default_role}"),
        Some(json!({"allow":0,"deny":512})),
        Some(&owner),
    )
    .await;
    assert_eq!(denied.status, StatusCode::NO_CONTENT, "{:?}", denied.json);
    let revoked = call(
        &api,
        Method::POST,
        &format!("/api/v1/channels/{channel_id}/voice-token"),
        None,
        Some(&owner),
    )
    .await;
    assert_eq!(revoked.status, StatusCode::NOT_FOUND);

    let direct = call(
        &api,
        Method::POST,
        "/api/v1/conversations",
        Some(json!({"type":"direct","username":"voiceoutsider"})),
        Some(&owner),
    )
    .await;
    assert_eq!(direct.status, StatusCode::CREATED);
    let conversation_id = direct.json["conversation"]["id"].as_str().unwrap();
    let non_member = call(
        &api,
        Method::POST,
        &format!("/api/v1/conversations/{conversation_id}/voice-token"),
        None,
        Some(&sign_in(&api, "voicethird").await),
    )
    .await;
    assert_eq!(non_member.status, StatusCode::NOT_FOUND);
    let direct_token = call(
        &api,
        Method::POST,
        &format!("/api/v1/conversations/{conversation_id}/voice-token"),
        None,
        Some(&owner),
    )
    .await;
    assert_eq!(
        direct_token.status,
        StatusCode::OK,
        "{:?}",
        direct_token.json
    );
    let direct_claims: serde_json::Value = serde_json::from_slice(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(
                direct_token.json["token"]
                    .as_str()
                    .unwrap()
                    .split('.')
                    .nth(1)
                    .unwrap(),
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        direct_claims["video"]["room"],
        format!("rustchat-conv-{conversation_id}")
    );
}

use base64::Engine;
