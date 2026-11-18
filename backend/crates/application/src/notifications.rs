//! Persistent in-app notifications.

use std::sync::Arc;

use rustchat_domain::identity::UserId;
use uuid::Uuid;

use crate::{
    error::AppError,
    ports::{NotificationRecord, NotificationRepository, WebPushDelivery},
};

pub struct NotificationService {
    repository: Arc<dyn NotificationRepository>,
    push: Option<Arc<dyn WebPushDelivery>>,
}

impl NotificationService {
    pub fn new(repository: Arc<dyn NotificationRepository>) -> Self {
        Self {
            repository,
            push: None,
        }
    }
    pub fn with_push_delivery(mut self, push: Arc<dyn WebPushDelivery>) -> Self {
        self.push = Some(push);
        self
    }
    pub fn push_public_key(&self) -> Option<String> {
        self.push.as_ref().map(|push| push.public_key())
    }

    pub async fn save_push_subscription(
        &self,
        user: UserId,
        endpoint: &str,
        p256dh: &str,
        auth_secret: &str,
    ) -> Result<Uuid, AppError> {
        if self.push.is_none() {
            return Err(AppError::WebPushUnavailable);
        }
        let host = endpoint
            .strip_prefix("https://")
            .and_then(|rest| rest.split('/').next())
            .unwrap_or("")
            .to_ascii_lowercase();
        let allowed = host == "updates.push.services.mozilla.com"
            || host == "fcm.googleapis.com"
            || host.ends_with(".fcm.googleapis.com")
            || host.ends_with(".push.apple.com")
            || host.ends_with(".notify.windows.com");
        let valid_key = |value: &str| {
            value.len() >= 16
                && value.len() <= 128
                && value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        };
        if !allowed || endpoint.len() > 2048 || !valid_key(p256dh) || !valid_key(auth_secret) {
            return Err(AppError::InvalidNotificationPreferences);
        }
        self.repository
            .save_push_subscription(user, endpoint, p256dh, auth_secret)
            .await
    }
    pub async fn delete_push_subscription(&self, user: UserId, id: Uuid) -> Result<(), AppError> {
        self.repository.delete_push_subscription(user, id).await
    }
    pub async fn dispatch_push(
        &self,
        user: UserId,
        kind: &str,
        source: Uuid,
    ) -> Result<(), AppError> {
        let Some(push) = self.push.as_ref() else {
            return Ok(());
        };
        if !self
            .repository
            .notification_exists(user, kind, source)
            .await?
        {
            return Ok(());
        }
        if self.repository.is_in_dnd_schedule(user).await? {
            return Ok(());
        }
        let subscriptions = self.repository.push_subscriptions(user).await?;
        let body = match kind {
            "direct_message" => "You have a new message.",
            "mention" => "You were mentioned.",
            "friend_request" => "You have a new friend request.",
            "community_event" => "There is a community update.",
            _ => "You have a new notification.",
        };
        let payload = serde_json::json!({"title":"RustChat","body":body,"url":"/"}).to_string();
        for subscription in subscriptions {
            match push.send(&subscription, &payload).await {
                Ok(false) => {
                    self.repository
                        .delete_push_subscription(user, subscription.id)
                        .await?
                }
                Ok(true) => {}
                Err(error) => {
                    tracing::warn!(%error, subscription_id = %subscription.id, "web push delivery failed")
                }
            }
        }
        Ok(())
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
    pub async fn community_event(
        &self,
        community: rustchat_domain::identity::CommunityId,
        actor: UserId,
        event_type: &str,
    ) -> Result<Vec<(UserId, Uuid)>, AppError> {
        if !matches!(
            event_type,
            "member_joined"
                | "member_left"
                | "member_removed"
                | "channel_created"
                | "channel_updated"
                | "channel_deleted"
        ) {
            return Err(AppError::InvalidNotificationPreferences);
        }
        let source = Uuid::now_v7();
        self.repository
            .create_community_event(community, actor, event_type, source)
            .await
    }
    pub async fn community_preferences(
        &self,
        user: UserId,
        community: rustchat_domain::identity::CommunityId,
    ) -> Result<crate::ports::CommunityNotificationPreferences, AppError> {
        self.repository.community_preferences(user, community).await
    }
    pub async fn set_community_preferences(
        &self,
        user: UserId,
        community: rustchat_domain::identity::CommunityId,
        member_events_enabled: bool,
        channel_events_enabled: bool,
    ) -> Result<crate::ports::CommunityNotificationPreferences, AppError> {
        self.repository
            .set_community_preferences(
                user,
                community,
                member_events_enabled,
                channel_events_enabled,
            )
            .await
    }
}
