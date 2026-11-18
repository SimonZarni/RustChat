//! Short-lived LiveKit room tokens. The API secret never leaves the server.

use axum::{
    Json,
    extract::{Path, State},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac, digest::KeyInit};
use rustchat_domain::{
    conversation::ConversationKind,
    identity::{ChannelId, ConversationId},
};
use serde::Serialize;
use sha2::Sha256;
use uuid::Uuid;

use crate::{ApiError, AppState, auth::AuthUser};

#[derive(Serialize)]
struct TokenClaims<'a> {
    iss: &'a str,
    sub: String,
    name: String,
    nbf: i64,
    exp: i64,
    video: VideoGrants,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VideoGrants {
    room_join: bool,
    room: String,
    can_publish: bool,
    can_subscribe: bool,
    can_publish_data: bool,
    can_publish_sources: [&'static str; 1],
}

#[derive(Serialize)]
pub(super) struct VoiceToken {
    url: String,
    token: String,
    expires_at: i64,
}

pub async fn token(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw_channel): Path<String>,
) -> Result<Json<VoiceToken>, ApiError> {
    let channel_id = Uuid::parse_str(&raw_channel)
        .map(ChannelId::from_uuid)
        .map_err(|_| ApiError::BadRequest(axum::http::StatusCode::BAD_REQUEST))?;
    if !state
        .communities
        .can_join_voice(auth.user.id, channel_id)
        .await?
    {
        return Err(rustchat_application::error::AppError::NotFound.into());
    }
    let config = state
        .voice
        .as_ref()
        .ok_or(rustchat_application::error::AppError::VoiceUnavailable)?;
    Ok(Json(issue_token(
        config,
        auth.user.id.as_uuid(),
        auth.user.username.to_string(),
        format!("rustchat-{}", channel_id.as_uuid()),
    )?))
}

pub async fn conversation_token(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw_conversation): Path<String>,
) -> Result<Json<VoiceToken>, ApiError> {
    let conversation_id = Uuid::parse_str(&raw_conversation)
        .map(ConversationId::from_uuid)
        .map_err(|_| ApiError::BadRequest(axum::http::StatusCode::BAD_REQUEST))?;
    let conversation = state
        .conversations
        .get(auth.user.id, conversation_id)
        .await?;
    if conversation.kind != ConversationKind::Direct && conversation.kind != ConversationKind::Group
    {
        return Err(rustchat_application::error::AppError::NotFound.into());
    }
    for community in state.communities.list(auth.user.id).await? {
        if state
            .communities
            .channels(auth.user.id, community.id)
            .await?
            .iter()
            .any(|channel| channel.conversation_id == conversation_id)
        {
            return Err(rustchat_application::error::AppError::NotFound.into());
        }
    }
    let config = state
        .voice
        .as_ref()
        .ok_or(rustchat_application::error::AppError::VoiceUnavailable)?;
    Ok(Json(issue_token(
        config,
        auth.user.id.as_uuid(),
        auth.user.username.to_string(),
        format!("rustchat-conv-{}", conversation_id.as_uuid()),
    )?))
}

fn issue_token(
    config: &crate::VoiceConfig,
    user_id: Uuid,
    username: String,
    room: String,
) -> Result<VoiceToken, ApiError> {
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let expires_at = now + 600;
    let claims = TokenClaims {
        iss: &config.api_key,
        sub: user_id.to_string(),
        name: username,
        nbf: now - 5,
        exp: expires_at,
        video: VideoGrants {
            room_join: true,
            room,
            can_publish: true,
            can_subscribe: true,
            can_publish_data: false,
            can_publish_sources: ["microphone"],
        },
    };
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"HS256","typ":"JWT"}"#);
    let payload = URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&claims).map_err(rustchat_application::error::AppError::internal)?,
    );
    let signing_input = format!("{header}.{payload}");
    let mut mac = Hmac::<Sha256>::new_from_slice(config.api_secret.as_bytes())
        .map_err(rustchat_application::error::AppError::internal)?;
    mac.update(signing_input.as_bytes());
    let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    Ok(VoiceToken {
        url: config.url.clone(),
        token: format!("{signing_input}.{signature}"),
        expires_at,
    })
}
