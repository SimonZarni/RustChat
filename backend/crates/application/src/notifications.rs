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
}
