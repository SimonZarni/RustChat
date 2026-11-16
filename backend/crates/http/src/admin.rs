//! Instance administration endpoints.

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use rustchat_application::{
    error::AppError,
    ports::{AdminUser, AuditEvent, AuditPage, AuditUser},
};
use rustchat_domain::identity::AuditEventId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    ApiError, AppState,
    auth::{ApiJson, AuthUser},
};

#[derive(Serialize)]
struct AdminUserDto {
    id: Uuid,
    username: String,
    created_at: String,
    is_instance_admin: bool,
    deactivated_at: Option<String>,
    suspended_at: Option<String>,
    deleted_at: Option<String>,
}

impl From<AdminUser> for AdminUserDto {
    fn from(value: AdminUser) -> Self {
        Self {
            id: value.user.id.as_uuid(),
            username: value.user.username.as_str().to_owned(),
            created_at: value.user.created_at.to_string(),
            is_instance_admin: value.is_instance_admin,
            deactivated_at: value.deactivated_at.map(|date| date.to_string()),
            suspended_at: value.suspended_at.map(|date| date.to_string()),
            deleted_at: value.deleted_at.map(|date| date.to_string()),
        }
    }
}

#[derive(Serialize)]
pub struct UsersEnvelope {
    users: Vec<AdminUserDto>,
}

#[derive(Serialize)]
pub struct UserEnvelope {
    user: AdminUserDto,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetSuspension {
    suspended: bool,
}

#[derive(Deserialize)]
pub struct AuditQuery {
    before: Option<String>,
    limit: Option<usize>,
}

impl AuditQuery {
    pub(crate) fn parse(self) -> Result<(Option<AuditEventId>, usize), ApiError> {
        let before = self
            .before
            .map(|raw| Uuid::parse_str(&raw).map(AuditEventId::from_uuid))
            .transpose()
            .map_err(|_| ApiError::BadRequest(StatusCode::BAD_REQUEST))?;
        Ok((before, self.limit.unwrap_or(50)))
    }
}

#[derive(Serialize)]
struct AuditUserDto {
    id: Uuid,
    username: String,
}

impl From<AuditUser> for AuditUserDto {
    fn from(user: AuditUser) -> Self {
        Self {
            id: user.id.as_uuid(),
            username: user.username,
        }
    }
}

#[derive(Serialize)]
struct AuditEventDto {
    id: Uuid,
    event_type: String,
    actor: Option<AuditUserDto>,
    target: Option<AuditUserDto>,
    details: serde_json::Value,
    #[serde(with = "time::serde::rfc3339")]
    created_at: time::OffsetDateTime,
}

impl From<AuditEvent> for AuditEventDto {
    fn from(event: AuditEvent) -> Self {
        Self {
            id: event.id.as_uuid(),
            event_type: event.event_type,
            actor: event.actor.map(Into::into),
            target: event.target.map(Into::into),
            details: event.details,
            created_at: event.created_at,
        }
    }
}

#[derive(Serialize)]
pub struct AuditEnvelope {
    events: Vec<AuditEventDto>,
    next_cursor: Option<Uuid>,
}

impl From<AuditPage> for AuditEnvelope {
    fn from(page: AuditPage) -> Self {
        Self {
            events: page.events.into_iter().map(Into::into).collect(),
            next_cursor: page.next_cursor.map(|id| id.as_uuid()),
        }
    }
}

pub async fn audit_log(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Query(query): Query<AuditQuery>,
) -> Result<Json<AuditEnvelope>, ApiError> {
    let (before, limit) = query.parse()?;
    let page = state
        .auth
        .list_instance_audit(auth.user.id, before, limit)
        .await?;
    Ok(Json(page.into()))
}

pub async fn list_users(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
) -> Result<Json<UsersEnvelope>, ApiError> {
    let users = state.auth.list_instance_users(auth.user.id).await?;
    Ok(Json(UsersEnvelope {
        users: users.into_iter().map(Into::into).collect(),
    }))
}

pub async fn set_suspension(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(raw_user_id): Path<String>,
    ApiJson(body): ApiJson<SetSuspension>,
) -> Result<Json<UserEnvelope>, ApiError> {
    let user_id = Uuid::parse_str(&raw_user_id)
        .map(rustchat_domain::identity::UserId::from_uuid)
        .map_err(|_| ApiError::App(AppError::NotFound))?;
    let user = state
        .auth
        .set_user_suspended(auth.user.id, user_id, body.suspended)
        .await?;
    Ok(Json(UserEnvelope { user: user.into() }))
}
