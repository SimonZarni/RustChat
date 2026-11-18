use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use rustchat_application::error::AppError;
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
