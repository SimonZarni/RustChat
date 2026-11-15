//! Instance administration endpoints.

use axum::{
    Json,
    extract::{Path, State},
};
use rustchat_application::{error::AppError, ports::AdminUser};
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
