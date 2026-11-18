//! Friend requests and user blocking.

use std::sync::Arc;

use rustchat_domain::identity::{UserId, Username};
use uuid::Uuid;

use crate::{
    error::AppError,
    ports::{FriendRequest, SocialRepository, SocialUser},
};

pub struct SocialService {
    repository: Arc<dyn SocialRepository>,
}

impl SocialService {
    pub fn new(repository: Arc<dyn SocialRepository>) -> Self {
        Self { repository }
    }
    pub async fn requests(
        &self,
        user: UserId,
        incoming: bool,
    ) -> Result<Vec<FriendRequest>, AppError> {
        self.repository.requests(user, incoming).await
    }
    pub async fn friends(&self, user: UserId) -> Result<Vec<SocialUser>, AppError> {
        self.repository.friends(user).await
    }
    pub async fn blocks(&self, user: UserId) -> Result<Vec<SocialUser>, AppError> {
        self.repository.blocks(user).await
    }
    pub async fn send(&self, user: UserId, username: &str) -> Result<(Uuid, UserId), AppError> {
        self.repository
            .send_request(user, &Username::parse(username)?)
            .await
    }
    pub async fn accept(&self, user: UserId, id: Uuid) -> Result<(), AppError> {
        self.repository.accept_request(user, id).await
    }
    pub async fn reject(&self, user: UserId, id: Uuid) -> Result<(), AppError> {
        self.repository.reject_request(user, id).await
    }
    pub async fn cancel(&self, user: UserId, id: Uuid) -> Result<(), AppError> {
        self.repository.cancel_request(user, id).await
    }
    pub async fn remove_friend(&self, user: UserId, friend: UserId) -> Result<(), AppError> {
        self.repository.remove_friend(user, friend).await
    }
    pub async fn block(&self, user: UserId, username: &str) -> Result<(), AppError> {
        self.repository
            .block(user, &Username::parse(username)?)
            .await
    }
    pub async fn unblock(&self, user: UserId, blocked: UserId) -> Result<(), AppError> {
        self.repository.unblock(user, blocked).await
    }
}
