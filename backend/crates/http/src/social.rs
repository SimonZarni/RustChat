use crate::{
    ApiError, AppState,
    auth::{ApiJson, AuthUser},
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use rustchat_application::{
    error::AppError,
    ports::{FriendRequest, SocialUser},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize)]
pub struct UsersEnvelope {
    pub users: Vec<SocialUser>,
}
#[derive(Serialize)]
pub struct RequestsEnvelope {
    pub requests: Vec<FriendRequest>,
}
#[derive(Serialize)]
pub struct RequestEnvelope {
    pub id: Uuid,
}
#[derive(Deserialize)]
pub struct Direction {
    pub direction: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsernameBody {
    pub username: String,
}
fn service(s: &AppState) -> Result<&rustchat_application::social::SocialService, ApiError> {
    s.social.as_deref().ok_or(ApiError::App(AppError::internal(
        "social service unavailable",
    )))
}
pub async fn friends(
    State(s): State<AppState>,
    AuthUser(a): AuthUser,
) -> Result<Json<UsersEnvelope>, ApiError> {
    Ok(Json(UsersEnvelope {
        users: service(&s)?.friends(a.user.id).await?,
    }))
}
pub async fn blocks(
    State(s): State<AppState>,
    AuthUser(a): AuthUser,
) -> Result<Json<UsersEnvelope>, ApiError> {
    Ok(Json(UsersEnvelope {
        users: service(&s)?.blocks(a.user.id).await?,
    }))
}
pub async fn requests(
    State(s): State<AppState>,
    AuthUser(a): AuthUser,
    Query(q): Query<Direction>,
) -> Result<Json<RequestsEnvelope>, ApiError> {
    let incoming = match q.direction.as_deref().unwrap_or("incoming") {
        "incoming" => true,
        "outgoing" => false,
        _ => return Err(ApiError::App(AppError::InvalidConversationRequest)),
    };
    Ok(Json(RequestsEnvelope {
        requests: service(&s)?.requests(a.user.id, incoming).await?,
    }))
}
pub async fn send_request(
    State(s): State<AppState>,
    AuthUser(a): AuthUser,
    ApiJson(b): ApiJson<UsernameBody>,
) -> Result<Json<RequestEnvelope>, ApiError> {
    let (id, recipient) = service(&s)?.send(a.user.id, &b.username).await?;
    s.publish_notification(crate::NotificationEvent {
        event_id: id,
        recipient_id: recipient.as_uuid(),
        kind: "friend_request".to_owned(),
        occurred_at: time::OffsetDateTime::now_utc(),
    });
    Ok(Json(RequestEnvelope { id }))
}
pub async fn accept_request(
    State(s): State<AppState>,
    AuthUser(a): AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    service(&s)?.accept(a.user.id, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
pub async fn reject_request(
    State(s): State<AppState>,
    AuthUser(a): AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    service(&s)?.reject(a.user.id, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
pub async fn cancel_request(
    State(s): State<AppState>,
    AuthUser(a): AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    service(&s)?.cancel(a.user.id, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
pub async fn remove_friend(
    State(s): State<AppState>,
    AuthUser(a): AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    service(&s)?
        .remove_friend(a.user.id, rustchat_domain::identity::UserId::from_uuid(id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
pub async fn block(
    State(s): State<AppState>,
    AuthUser(a): AuthUser,
    ApiJson(b): ApiJson<UsernameBody>,
) -> Result<StatusCode, ApiError> {
    service(&s)?.block(a.user.id, &b.username).await?;
    Ok(StatusCode::NO_CONTENT)
}
pub async fn unblock(
    State(s): State<AppState>,
    AuthUser(a): AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    service(&s)?
        .unblock(a.user.id, rustchat_domain::identity::UserId::from_uuid(id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
