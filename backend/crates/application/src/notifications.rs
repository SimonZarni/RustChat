//! Persistent in-app notifications.

use std::sync::Arc;

use rustchat_domain::identity::UserId;
use uuid::Uuid;

use crate::{
    error::AppError,
    ports::{NotificationRecord, NotificationRepository},
};

pub struct NotificationService {
    repository: Arc<dyn NotificationRepository>,
}

impl NotificationService {
    pub fn new(repository: Arc<dyn NotificationRepository>) -> Self {
        Self { repository }
    }
    pub async fn inbox(
        &self,
        user: UserId,
        limit: usize,
    ) -> Result<(Vec<NotificationRecord>, i64), AppError> {
        let limit = limit.clamp(1, 100);
        let notifications = self.repository.list(user, limit).await?;
        let unread = self.repository.unread_count(user).await?;
        Ok((notifications, unread))
    }
    pub async fn mark_read(&self, user: UserId, id: Uuid) -> Result<(), AppError> {
        self.repository.mark_read(user, id).await
    }
    pub async fn mark_all_read(&self, user: UserId) -> Result<(), AppError> {
        self.repository.mark_all_read(user).await
    }
    pub async fn preferences(
        &self,
        user: UserId,
        conversation: rustchat_domain::identity::ConversationId,
    ) -> Result<crate::ports::ConversationNotificationPreferences, AppError> {
        self.repository.preferences(user, conversation).await
    }
    pub async fn set_preferences(
        &self,
        user: UserId,
        conversation: rustchat_domain::identity::ConversationId,
        messages_enabled: bool,
        mentions_enabled: bool,
        muted: bool,
        muted_until: Option<time::OffsetDateTime>,
    ) -> Result<crate::ports::ConversationNotificationPreferences, AppError> {
        if muted && muted_until.is_some()
            || muted_until.is_some_and(|until| until <= time::OffsetDateTime::now_utc())
        {
            return Err(AppError::InvalidNotificationPreferences);
        }
        self.repository
            .set_preferences(
                user,
                conversation,
                messages_enabled,
                mentions_enabled,
                muted,
                muted_until,
            )
            .await
    }
    pub async fn dnd_schedule(&self, user: UserId) -> Result<crate::ports::DndSchedule, AppError> {
        self.repository.dnd_schedule(user).await
    }
    pub async fn set_dnd_schedule(
        &self,
        user: UserId,
        schedule: crate::ports::DndSchedule,
    ) -> Result<crate::ports::DndSchedule, AppError> {
        let valid_time = |value: &str| {
            value.len() == 5
                && value.as_bytes().get(2) == Some(&b':')
                && value
                    .get(..2)
                    .and_then(|v| v.parse::<u8>().ok())
                    .is_some_and(|h| h < 24)
                && value
                    .get(3..)
                    .and_then(|v| v.parse::<u8>().ok())
                    .is_some_and(|m| m < 60)
        };
        let unique_days = schedule
            .weekdays
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            == schedule.weekdays.len();
        if !valid_time(&schedule.start_time)
            || !valid_time(&schedule.end_time)
            || schedule.start_time == schedule.end_time
            || schedule.timezone.is_empty()
            || schedule.timezone.len() > 64
            || schedule.weekdays.is_empty()
            || schedule.weekdays.len() > 7
            || !unique_days
            || schedule.weekdays.iter().any(|day| !(1..=7).contains(day))
        {
            return Err(AppError::InvalidNotificationPreferences);
        }
        self.repository.set_dnd_schedule(user, schedule).await
    }
}
