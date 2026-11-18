//! Authorization and signed URL endpoints for message attachments.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use rustchat_domain::identity::{AttachmentId, MessageId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    ApiError, AppState,
    auth::{ApiJson, AuthUser},
    messages::parse_id,
};

fn service(
    state: &AppState,
) -> Result<&rustchat_application::attachments::AttachmentService, ApiError> {
    state
        .attachments
        .as_deref()
        .ok_or(rustchat_application::error::AppError::AttachmentStorageUnavailable.into())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReserveBody {
    file_name: String,
    content_type: String,
    byte_size: i64,
}

#[derive(Serialize)]
pub struct UploadEnvelope {
    attachment_id: Uuid,
    upload_url: String,
    required_content_type: &'static str,
    #[serde(with = "time::serde::rfc3339")]
    expires_at: time::OffsetDateTime,
}

pub async fn reserve(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw): Path<String>,
    ApiJson(body): ApiJson<ReserveBody>,
) -> Result<(StatusCode, Json<UploadEnvelope>), ApiError> {
    let reservation = service(&state)?
        .reserve(
            auth.user.id,
            parse_id(&raw)?,
            &body.file_name,
            &body.content_type,
            body.byte_size,
        )
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(UploadEnvelope {
            attachment_id: reservation.attachment_id.as_uuid(),
            upload_url: reservation.upload_url,
            required_content_type: reservation.required_content_type,
            expires_at: reservation.expires_at,
        }),
    ))
}

#[derive(Serialize)]
pub struct AttachmentDto {
    id: Uuid,
    message_id: Option<Uuid>,
    file_name: String,
    media_type: String,
    byte_size: i64,
    #[serde(with = "time::serde::rfc3339")]
    created_at: time::OffsetDateTime,
}

impl From<rustchat_domain::attachment::Attachment> for AttachmentDto {
    fn from(item: rustchat_domain::attachment::Attachment) -> Self {
        Self {
            id: item.id.as_uuid(),
            message_id: item.message_id.map(|id| id.as_uuid()),
            file_name: item.file_name,
            media_type: item.media_type,
            byte_size: item.byte_size,
            created_at: item.created_at,
        }
    }
}

#[derive(Serialize)]
pub struct CompleteEnvelope {
    attachment: AttachmentDto,
}

pub async fn complete(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw): Path<String>,
) -> Result<Json<CompleteEnvelope>, ApiError> {
    let id = AttachmentId::from_uuid(
        Uuid::parse_str(&raw).map_err(|_| ApiError::BadRequest(StatusCode::BAD_REQUEST))?,
    );
    let attachment = service(&state)?.complete(auth.user.id, id).await?;
    Ok(Json(CompleteEnvelope {
        attachment: attachment.into(),
    }))
}

pub async fn cancel(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = AttachmentId::from_uuid(
        Uuid::parse_str(&raw).map_err(|_| ApiError::BadRequest(StatusCode::BAD_REQUEST))?,
    );
    service(&state)?.delete_unattached(auth.user.id, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
pub struct DownloadEnvelope {
    attachment: AttachmentDto,
    download_url: String,
    #[serde(with = "time::serde::rfc3339")]
    expires_at: time::OffsetDateTime,
}

pub async fn download(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw): Path<String>,
) -> Result<Json<DownloadEnvelope>, ApiError> {
    let id = AttachmentId::from_uuid(
        Uuid::parse_str(&raw).map_err(|_| ApiError::BadRequest(StatusCode::BAD_REQUEST))?,
    );
    let download = service(&state)?.download(auth.user.id, id).await?;
    Ok(Json(DownloadEnvelope {
        attachment: download.attachment.into(),
        download_url: download.download_url,
        expires_at: download.expires_at,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkBody {
    attachment_ids: Vec<Uuid>,
}

#[derive(Serialize)]
pub struct LinkedEnvelope {
    attachments: Vec<AttachmentDto>,
}

pub async fn link_to_message(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path((raw_conversation, raw_message)): Path<(String, String)>,
    ApiJson(body): ApiJson<LinkBody>,
) -> Result<Json<LinkedEnvelope>, ApiError> {
    let conversation = parse_id(&raw_conversation)?;
    let message = MessageId::from_uuid(
        Uuid::parse_str(&raw_message).map_err(|_| ApiError::BadRequest(StatusCode::BAD_REQUEST))?,
    );
    let ids = body
        .attachment_ids
        .into_iter()
        .map(AttachmentId::from_uuid)
        .collect::<Vec<_>>();
    let attachments = service(&state)?;
    attachments
        .attach_to_message(auth.user.id, conversation, message, &ids)
        .await?;
    let records = attachments
        .list_for_messages(auth.user.id, conversation, &[message])
        .await?;
    Ok(Json(LinkedEnvelope {
        attachments: records.into_iter().map(Into::into).collect(),
    }))
}

pub async fn list_for_message(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path((raw_conversation, raw_message)): Path<(String, String)>,
) -> Result<Json<LinkedEnvelope>, ApiError> {
    let records = service(&state)?
        .list_for_messages(
            auth.user.id,
            parse_id(&raw_conversation)?,
            &[MessageId::from_uuid(
                Uuid::parse_str(&raw_message)
                    .map_err(|_| ApiError::BadRequest(StatusCode::BAD_REQUEST))?,
            )],
        )
        .await?;
    Ok(Json(LinkedEnvelope {
        attachments: records.into_iter().map(Into::into).collect(),
    }))
}
