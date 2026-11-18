//! Authenticated conversation endpoints.

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use rustchat_domain::{
    conversation::{Conversation, ConversationKind},
    identity::{ConversationId, UserId},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    ApiError, AppState,
    auth::{ApiJson, AuthUser},
};

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CreateConversation {
    Direct {
        username: String,
    },
    Group {
        usernames: Vec<String>,
        /// Optional display name, 1-80 characters.
        #[serde(default)]
        name: Option<String>,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddMember {
    username: String,
}

#[derive(Deserialize)]
pub struct ListQuery {
    before: Option<String>,
    limit: Option<usize>,
}

#[derive(Serialize)]
pub struct ConversationDto {
    id: Uuid,
    kind: &'static str,
    /// Group name; null for direct conversations and unnamed groups.
    name: Option<String>,
    created_by: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    created_at: time::OffsetDateTime,
    members: Vec<Uuid>,
}

impl From<Conversation> for ConversationDto {
    fn from(c: Conversation) -> Self {
        Self {
            id: c.id.as_uuid(),
            kind: match c.kind {
                ConversationKind::Direct => "direct",
                ConversationKind::Group => "group",
            },
            name: c.name.map(|name| name.as_str().to_owned()),
            created_by: c.created_by.as_uuid(),
            created_at: c.created_at,
            members: c.members.into_iter().map(|u| u.as_uuid()).collect(),
        }
    }
}

#[derive(Serialize)]
pub struct ConversationEnvelope {
    conversation: ConversationDto,
}
#[derive(Serialize)]
pub struct ConversationsEnvelope {
    conversations: Vec<ConversationDto>,
    next_cursor: Option<Uuid>,
}

#[derive(Serialize)]
pub struct MembersEnvelope {
    members: Vec<MemberDto>,
}

#[derive(Serialize)]
struct MemberDto {
    id: Uuid,
    username: String,
    #[serde(with = "time::serde::rfc3339")]
    created_at: time::OffsetDateTime,
}

pub async fn create(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    ApiJson(body): ApiJson<CreateConversation>,
) -> Result<(StatusCode, Json<ConversationEnvelope>), ApiError> {
    let conversation = match body {
        CreateConversation::Direct { username } => {
            state
                .conversations
                .create_direct(auth.user.id, &username)
                .await?
        }
        CreateConversation::Group { usernames, name } => {
            state
                .conversations
                .create_group(auth.user.id, name.as_deref(), &usernames)
                .await?
        }
    };
    Ok((
        StatusCode::CREATED,
        Json(ConversationEnvelope {
            conversation: conversation.into(),
        }),
    ))
}

pub async fn list(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Query(query): Query<ListQuery>,
) -> Result<Json<ConversationsEnvelope>, ApiError> {
    let before = query
        .before
        .map(|raw| Uuid::parse_str(&raw).map(ConversationId::from_uuid))
        .transpose()
        .map_err(|_| ApiError::BadRequest(StatusCode::BAD_REQUEST))?;
    let page = state
        .conversations
        .list(auth.user.id, before, query.limit.unwrap_or(50))
        .await?;
    Ok(Json(ConversationsEnvelope {
        conversations: page.conversations.into_iter().map(Into::into).collect(),
        next_cursor: page.next_cursor.map(|id| id.as_uuid()),
    }))
}

pub async fn get(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw): Path<String>,
) -> Result<Json<ConversationEnvelope>, ApiError> {
    let id = parse_id(&raw)?;
    let conversation = state.conversations.get(auth.user.id, id).await?;
    Ok(Json(ConversationEnvelope {
        conversation: conversation.into(),
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetNotificationPreferences {
    messages_enabled: bool,
    mentions_enabled: bool,
    muted: bool,
    #[serde(default, with = "time::serde::rfc3339::option")]
    muted_until: Option<time::OffsetDateTime>,
}

pub async fn notification_preferences(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw): Path<String>,
) -> Result<Json<rustchat_application::ports::ConversationNotificationPreferences>, ApiError> {
    let id = parse_id(&raw)?;
    state.conversations.get(auth.user.id, id).await?;
    let service = state.notifications.as_deref().ok_or(ApiError::App(
        rustchat_application::error::AppError::internal("notification service unavailable"),
    ))?;
    Ok(Json(service.preferences(auth.user.id, id).await?))
}

pub async fn set_notification_preferences(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw): Path<String>,
    ApiJson(body): ApiJson<SetNotificationPreferences>,
) -> Result<Json<rustchat_application::ports::ConversationNotificationPreferences>, ApiError> {
    let id = parse_id(&raw)?;
    state.conversations.get(auth.user.id, id).await?;
    let service = state.notifications.as_deref().ok_or(ApiError::App(
        rustchat_application::error::AppError::internal("notification service unavailable"),
    ))?;
    Ok(Json(
        service
            .set_preferences(
                auth.user.id,
                id,
                body.messages_enabled,
                body.mentions_enabled,
                body.muted,
                body.muted_until,
            )
            .await?,
    ))
}

pub async fn members(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw): Path<String>,
) -> Result<Json<MembersEnvelope>, ApiError> {
    let conversation_id = parse_id(&raw)?;
    let members = state
        .conversations
        .members(auth.user.id, conversation_id)
        .await?
        .into_iter()
        .map(|user| MemberDto {
            id: user.id.as_uuid(),
            username: user.username.to_string(),
            created_at: user.created_at,
        })
        .collect();
    Ok(Json(MembersEnvelope { members }))
}

pub async fn add_member(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw): Path<String>,
    ApiJson(body): ApiJson<AddMember>,
) -> Result<StatusCode, ApiError> {
    state
        .conversations
        .add_member(auth.user.id, parse_id(&raw)?, &body.username)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_member(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path((conversation, member)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let member = Uuid::parse_str(&member)
        .map(UserId::from_uuid)
        .map_err(|_| ApiError::BadRequest(StatusCode::BAD_REQUEST))?;
    state
        .conversations
        .remove_member(auth.user.id, parse_id(&conversation)?, member)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

fn parse_id(raw: &str) -> Result<ConversationId, ApiError> {
    Uuid::parse_str(raw)
        .map(ConversationId::from_uuid)
        .map_err(|_| ApiError::BadRequest(StatusCode::BAD_REQUEST))
}
