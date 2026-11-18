//! Authenticated message sending and history endpoints.

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use rustchat_domain::{
    identity::{ConversationId, MessageId},
    message::Message,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    ApiError, AppState,
    auth::{ApiJson, AuthUser},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SendMessage {
    client_message_id: Uuid,
    reply_to_message_id: Option<Uuid>,
    content: String,
    #[serde(default)]
    attachment_ids: Vec<Uuid>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateReadCursor {
    sequence: i64,
}

#[derive(Serialize)]
pub struct ReadCursorDto {
    user_id: Uuid,
    sequence: i64,
    #[serde(with = "time::serde::rfc3339")]
    updated_at: time::OffsetDateTime,
}

impl From<rustchat_domain::message::ReadCursor> for ReadCursorDto {
    fn from(cursor: rustchat_domain::message::ReadCursor) -> Self {
        Self {
            user_id: cursor.user_id.as_uuid(),
            sequence: cursor.sequence,
            updated_at: cursor.updated_at,
        }
    }
}

#[derive(Serialize)]
pub struct ReadCursorEnvelope {
    cursor: ReadCursorDto,
}

#[derive(Serialize)]
pub struct ReadCursorsEnvelope {
    cursors: Vec<ReadCursorDto>,
}

#[derive(Serialize)]
pub struct DeliveryCursorDto {
    user_id: Uuid,
    sequence: i64,
    #[serde(with = "time::serde::rfc3339")]
    updated_at: time::OffsetDateTime,
}

impl From<rustchat_domain::message::DeliveryCursor> for DeliveryCursorDto {
    fn from(cursor: rustchat_domain::message::DeliveryCursor) -> Self {
        Self {
            user_id: cursor.user_id.as_uuid(),
            sequence: cursor.sequence,
            updated_at: cursor.updated_at,
        }
    }
}

#[derive(Serialize)]
pub struct DeliveryCursorEnvelope {
    cursor: DeliveryCursorDto,
}

#[derive(Serialize)]
pub struct DeliveryCursorsEnvelope {
    cursors: Vec<DeliveryCursorDto>,
}

#[derive(Serialize)]
pub struct MessageReactionDto {
    message_id: Uuid,
    emoji: String,
    count: i64,
    reacted_by_me: bool,
}

impl From<rustchat_domain::message::MessageReaction> for MessageReactionDto {
    fn from(reaction: rustchat_domain::message::MessageReaction) -> Self {
        Self {
            message_id: reaction.message_id.as_uuid(),
            emoji: reaction.emoji,
            count: reaction.count,
            reacted_by_me: reaction.reacted_by_me,
        }
    }
}

#[derive(Serialize)]
pub struct ReactionsEnvelope {
    reactions: Vec<MessageReactionDto>,
}

#[derive(Serialize)]
pub struct ReactionMutationEnvelope {
    sequence: i64,
    changed: bool,
    reactions: Vec<MessageReactionDto>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetReaction {
    emoji: String,
}

#[derive(Deserialize)]
pub struct ReactionsQuery {
    message_ids: Option<String>,
}

pub async fn advance_read_cursor(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw): Path<String>,
    ApiJson(body): ApiJson<UpdateReadCursor>,
) -> Result<Json<ReadCursorEnvelope>, ApiError> {
    let conversation = parse_id(&raw)?;
    let cursor = state
        .messages
        .advance_read_cursor(auth.user.id, conversation, body.sequence)
        .await?;
    state.publish_read_cursor(crate::ReadCursorEvent {
        conversation_id: conversation.as_uuid(),
        user_id: cursor.user_id.as_uuid(),
        sequence: cursor.sequence,
        occurred_at: cursor.updated_at,
    });
    Ok(Json(ReadCursorEnvelope {
        cursor: cursor.into(),
    }))
}

pub async fn read_cursors(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw): Path<String>,
) -> Result<Json<ReadCursorsEnvelope>, ApiError> {
    let cursors = state
        .messages
        .read_cursors(auth.user.id, parse_id(&raw)?)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(ReadCursorsEnvelope { cursors }))
}

pub async fn advance_delivery_cursor(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw): Path<String>,
    ApiJson(body): ApiJson<UpdateReadCursor>,
) -> Result<Json<DeliveryCursorEnvelope>, ApiError> {
    let conversation = parse_id(&raw)?;
    let cursor = state
        .messages
        .advance_delivery_cursor(auth.user.id, conversation, body.sequence)
        .await?;
    if cursor.advanced {
        state.publish_delivery_cursor(crate::DeliveryCursorEvent {
            conversation_id: conversation.as_uuid(),
            user_id: cursor.user_id.as_uuid(),
            sequence: cursor.sequence,
            occurred_at: cursor.updated_at,
        });
    }
    Ok(Json(DeliveryCursorEnvelope {
        cursor: cursor.into(),
    }))
}

pub async fn delivery_cursors(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw): Path<String>,
) -> Result<Json<DeliveryCursorsEnvelope>, ApiError> {
    let cursors = state
        .messages
        .delivery_cursors(auth.user.id, parse_id(&raw)?)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(DeliveryCursorsEnvelope { cursors }))
}

pub async fn list_reactions(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw): Path<String>,
    Query(query): Query<ReactionsQuery>,
) -> Result<Json<ReactionsEnvelope>, ApiError> {
    let messages = query
        .message_ids
        .unwrap_or_default()
        .split(',')
        .filter(|value| !value.is_empty())
        .map(parse_message_id)
        .collect::<Result<Vec<_>, _>>()?;
    if messages.len() > 100 {
        return Err(ApiError::BadRequest(StatusCode::BAD_REQUEST));
    }
    let reactions = state
        .messages
        .list_reactions(auth.user.id, parse_id(&raw)?, &messages)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(ReactionsEnvelope { reactions }))
}

pub async fn add_reaction(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path((conversation, message)): Path<(String, String)>,
    ApiJson(body): ApiJson<SetReaction>,
) -> Result<Json<ReactionMutationEnvelope>, ApiError> {
    set_reaction(state, auth.user.id, conversation, message, body.emoji, true).await
}

pub async fn remove_reaction(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path((conversation, message)): Path<(String, String)>,
    ApiJson(body): ApiJson<SetReaction>,
) -> Result<Json<ReactionMutationEnvelope>, ApiError> {
    set_reaction(
        state,
        auth.user.id,
        conversation,
        message,
        body.emoji,
        false,
    )
    .await
}

async fn set_reaction(
    state: AppState,
    user_id: rustchat_domain::identity::UserId,
    raw_conversation: String,
    raw_message: String,
    emoji: String,
    added: bool,
) -> Result<Json<ReactionMutationEnvelope>, ApiError> {
    let conversation = parse_id(&raw_conversation)?;
    let message_id = parse_message_id(&raw_message)?;
    let mutation = state
        .messages
        .set_reaction(user_id, conversation, message_id, emoji.clone(), added)
        .await?;
    if mutation.changed {
        state.publish_reaction(crate::ReactionEvent {
            conversation_id: conversation.as_uuid(),
            message_id: message_id.as_uuid(),
            user_id: user_id.as_uuid(),
            sequence: mutation.sequence,
            emoji,
            added,
            occurred_at: time::OffsetDateTime::now_utc(),
        });
    }
    Ok(Json(ReactionMutationEnvelope {
        sequence: mutation.sequence,
        changed: mutation.changed,
        reactions: mutation.reactions.into_iter().map(Into::into).collect(),
    }))
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    before_sequence: Option<i64>,
    after_sequence: Option<i64>,
    limit: Option<usize>,
}

#[derive(Serialize)]
pub struct MessageDto {
    id: Uuid,
    conversation_id: Uuid,
    sender_id: Uuid,
    sequence: i64,
    client_message_id: Uuid,
    reply_to_message_id: Option<Uuid>,
    mentioned_user_ids: Vec<Uuid>,
    content: String,
    #[serde(with = "time::serde::rfc3339")]
    created_at: time::OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    edited_at: Option<time::OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    deleted_at: Option<time::OffsetDateTime>,
    attachments: Vec<crate::attachments::AttachmentDto>,
}

impl From<Message> for MessageDto {
    fn from(message: Message) -> Self {
        Self {
            id: message.id.as_uuid(),
            conversation_id: message.conversation_id.as_uuid(),
            sender_id: message.sender_id.as_uuid(),
            sequence: message.sequence,
            client_message_id: message.client_message_id,
            reply_to_message_id: message.reply_to_message_id.map(|id| id.as_uuid()),
            mentioned_user_ids: message
                .mentioned_user_ids
                .into_iter()
                .map(|id| id.as_uuid())
                .collect(),
            content: message.content.as_str().to_owned(),
            created_at: message.created_at,
            edited_at: message.edited_at,
            deleted_at: message.deleted_at,
            attachments: Vec::new(),
        }
    }
}

#[derive(Serialize)]
pub struct MessageEnvelope {
    message: MessageDto,
}
#[derive(Serialize)]
pub struct MessagesEnvelope {
    messages: Vec<MessageDto>,
    next_before: Option<i64>,
    next_after: Option<i64>,
}

pub async fn send(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw): Path<String>,
    ApiJson(body): ApiJson<SendMessage>,
) -> Result<(StatusCode, Json<MessageEnvelope>), ApiError> {
    let conversation_id = parse_id(&raw)?;
    let attachments = body
        .attachment_ids
        .into_iter()
        .map(rustchat_domain::identity::AttachmentId::from_uuid)
        .collect::<Vec<_>>();
    let message = state
        .messages
        .send_with_attachments(
            auth.user.id,
            conversation_id,
            MessageId::from_uuid(body.client_message_id),
            body.reply_to_message_id.map(MessageId::from_uuid),
            body.content,
            attachments,
        )
        .await?;
    let mut message_dto = MessageDto::from(message.clone());
    if let Some(attachments) = &state.attachments {
        message_dto.attachments = attachments
            .list_for_messages(auth.user.id, conversation_id, &[message.id])
            .await?
            .into_iter()
            .map(Into::into)
            .collect();
    }
    Ok((
        StatusCode::CREATED,
        Json(MessageEnvelope {
            message: message_dto,
        }),
    ))
}

pub async fn list(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw): Path<String>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<MessagesEnvelope>, ApiError> {
    if query.before_sequence.is_some_and(|sequence| sequence <= 0)
        || query.after_sequence.is_some_and(|sequence| sequence < 0)
        || (query.before_sequence.is_some() && query.after_sequence.is_some())
    {
        return Err(ApiError::BadRequest(StatusCode::BAD_REQUEST));
    }
    let page = state
        .messages
        .list(
            auth.user.id,
            parse_id(&raw)?,
            query.before_sequence,
            query.after_sequence,
            query.limit.unwrap_or(50),
        )
        .await?;
    let message_ids = page
        .messages
        .iter()
        .map(|message| message.id)
        .collect::<Vec<_>>();
    let attachments = if let Some(service) = &state.attachments {
        service
            .list_for_messages(auth.user.id, parse_id(&raw)?, &message_ids)
            .await?
    } else {
        Vec::new()
    };
    let mut by_message =
        std::collections::HashMap::<Uuid, Vec<crate::attachments::AttachmentDto>>::new();
    for attachment in attachments {
        if let Some(message_id) = attachment.message_id {
            by_message
                .entry(message_id.as_uuid())
                .or_default()
                .push(attachment.into());
        }
    }
    let messages = page
        .messages
        .into_iter()
        .map(|message| {
            let id = message.id.as_uuid();
            let mut dto = MessageDto::from(message);
            dto.attachments = by_message.remove(&id).unwrap_or_default();
            dto
        })
        .collect();
    Ok(Json(MessagesEnvelope {
        messages,
        next_before: page.next_before,
        next_after: page.next_after,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditMessage {
    content: String,
}

pub async fn edit(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path((conversation, message)): Path<(String, String)>,
    ApiJson(body): ApiJson<EditMessage>,
) -> Result<Json<MessageEnvelope>, ApiError> {
    let updated = state
        .messages
        .edit(
            auth.user.id,
            parse_id(&conversation)?,
            parse_message_id(&message)?,
            body.content,
        )
        .await?;
    state.publish_message(&updated);
    Ok(Json(MessageEnvelope {
        message: updated.into(),
    }))
}

pub async fn delete(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path((conversation, message)): Path<(String, String)>,
) -> Result<Json<MessageEnvelope>, ApiError> {
    let deleted = state
        .messages
        .delete(
            auth.user.id,
            parse_id(&conversation)?,
            parse_message_id(&message)?,
        )
        .await?;
    state.publish_message(&deleted);
    Ok(Json(MessageEnvelope {
        message: deleted.into(),
    }))
}

fn parse_message_id(raw: &str) -> Result<MessageId, ApiError> {
    Uuid::parse_str(raw)
        .map(MessageId::from_uuid)
        .map_err(|_| ApiError::BadRequest(StatusCode::BAD_REQUEST))
}

pub(crate) fn parse_id(raw: &str) -> Result<ConversationId, ApiError> {
    Uuid::parse_str(raw)
        .map(ConversationId::from_uuid)
        .map_err(|_| ApiError::BadRequest(StatusCode::BAD_REQUEST))
}
