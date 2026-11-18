use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use rustchat_application::error::AppError;
use rustchat_domain::identity::CommunityId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{ApiError, AppState, auth::AuthUser};

#[derive(Deserialize)]
pub struct InboxQuery {
    limit: Option<usize>,
}
#[derive(Serialize)]
pub struct InboxEnvelope {
    notifications: Vec<rustchat_application::ports::NotificationRecord>,
    unread_count: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DndScheduleBody {
    enabled: bool,
    start_time: String,
    end_time: String,
    timezone: String,
    weekdays: Vec<i16>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommunityPreferencesBody {
    member_events_enabled: bool,
    channel_events_enabled: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PushSubscriptionBody {
    endpoint: String,
    #[serde(default, rename = "expirationTime")]
    expiration_time: Option<serde_json::Value>,
    keys: PushSubscriptionKeys,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PushSubscriptionKeys {
    p256dh: String,
    auth: String,
}
#[derive(Serialize)]
pub struct PushConfigEnvelope {
    enabled: bool,
    public_key: Option<String>,
}
#[derive(Serialize)]
pub struct PushSubscriptionEnvelope {
    id: Uuid,
}
fn service(
    s: &AppState,
) -> Result<&rustchat_application::notifications::NotificationService, ApiError> {
    s.notifications
        .as_deref()
        .ok_or(ApiError::App(AppError::internal(
            "notification service unavailable",
        )))
}

pub async fn inbox(
    State(s): State<AppState>,
    AuthUser(a): AuthUser,
    Query(q): Query<InboxQuery>,
) -> Result<Json<InboxEnvelope>, ApiError> {
    let (notifications, unread_count) =
        service(&s)?.inbox(a.user.id, q.limit.unwrap_or(50)).await?;
    Ok(Json(InboxEnvelope {
        notifications,
        unread_count,
    }))
}
pub async fn mark_read(
    State(s): State<AppState>,
    AuthUser(a): AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    service(&s)?.mark_read(a.user.id, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
pub async fn mark_all_read(
    State(s): State<AppState>,
    AuthUser(a): AuthUser,
) -> Result<StatusCode, ApiError> {
    service(&s)?.mark_all_read(a.user.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn dnd_schedule(
    State(s): State<AppState>,
    AuthUser(a): AuthUser,
) -> Result<Json<rustchat_application::ports::DndSchedule>, ApiError> {
    Ok(Json(service(&s)?.dnd_schedule(a.user.id).await?))
}

pub async fn set_dnd_schedule(
    State(s): State<AppState>,
    AuthUser(a): AuthUser,
    crate::auth::ApiJson(body): crate::auth::ApiJson<DndScheduleBody>,
) -> Result<Json<rustchat_application::ports::DndSchedule>, ApiError> {
    Ok(Json(
        service(&s)?
            .set_dnd_schedule(
                a.user.id,
                rustchat_application::ports::DndSchedule {
                    enabled: body.enabled,
                    start_time: body.start_time,
                    end_time: body.end_time,
                    timezone: body.timezone,
                    weekdays: body.weekdays,
                },
            )
            .await?,
    ))
}

pub async fn community_preferences(
    State(s): State<AppState>,
    AuthUser(a): AuthUser,
    Path(raw): Path<String>,
) -> Result<Json<rustchat_application::ports::CommunityNotificationPreferences>, ApiError> {
    let id = Uuid::parse_str(&raw)
        .map(CommunityId::from_uuid)
        .map_err(|_| ApiError::BadRequest(StatusCode::BAD_REQUEST))?;
    Ok(Json(
        service(&s)?.community_preferences(a.user.id, id).await?,
    ))
}

pub async fn set_community_preferences(
    State(s): State<AppState>,
    AuthUser(a): AuthUser,
    Path(raw): Path<String>,
    crate::auth::ApiJson(body): crate::auth::ApiJson<CommunityPreferencesBody>,
) -> Result<Json<rustchat_application::ports::CommunityNotificationPreferences>, ApiError> {
    let id = Uuid::parse_str(&raw)
        .map(CommunityId::from_uuid)
        .map_err(|_| ApiError::BadRequest(StatusCode::BAD_REQUEST))?;
    Ok(Json(
        service(&s)?
            .set_community_preferences(
                a.user.id,
                id,
                body.member_events_enabled,
                body.channel_events_enabled,
            )
            .await?,
    ))
}

pub async fn push_config(
    State(s): State<AppState>,
    AuthUser(_): AuthUser,
) -> Json<PushConfigEnvelope> {
    let public_key = s
        .notifications
        .as_deref()
        .and_then(|service| service.push_public_key());
    Json(PushConfigEnvelope {
        enabled: public_key.is_some(),
        public_key,
    })
}

pub async fn save_push_subscription(
    State(s): State<AppState>,
    AuthUser(a): AuthUser,
    crate::auth::ApiJson(body): crate::auth::ApiJson<PushSubscriptionBody>,
) -> Result<(StatusCode, Json<PushSubscriptionEnvelope>), ApiError> {
    let id = service(&s)?
        .save_push_subscription(
            a.user.id,
            &body.endpoint,
            &body.keys.p256dh,
            &body.keys.auth,
        )
        .await?;
    let _ = body.expiration_time;
    Ok((StatusCode::CREATED, Json(PushSubscriptionEnvelope { id })))
}

pub async fn delete_push_subscription(
    State(s): State<AppState>,
    AuthUser(a): AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    service(&s)?.delete_push_subscription(a.user.id, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
