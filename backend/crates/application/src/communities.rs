//! Community membership, text-channel and role workflows.

use std::sync::Arc;

use rustchat_domain::{
    community::{Category, Channel, Community, Role},
    identity::{CategoryId, ChannelId, CommunityId, RoleId, User, UserId},
};
use time::Duration;

use crate::{
    error::AppError,
    ports::{Clock, CommunityRepository, TokenService},
};

pub const VIEW: u64 = 1;
pub const MANAGE_COMMUNITY: u64 = 2;
pub const MANAGE_CHANNELS: u64 = 4;
pub const MANAGE_ROLES: u64 = 8;
pub const MANAGE_MEMBERS: u64 = 16;
pub const CREATE_INVITES: u64 = 32;
pub const SEND_MESSAGES: u64 = 64;
pub const READ_MESSAGES: u64 = 128;
pub const MANAGE_MESSAGES: u64 = 256;
pub const ALL_PERMISSIONS: u64 = 511;

#[derive(Clone, Copy)]
pub struct CommunityServiceConfig {
    pub max_invite_ttl: Duration,
}

pub struct CommunityService {
    repo: Arc<dyn CommunityRepository>,
    tokens: Arc<dyn TokenService>,
    clock: Arc<dyn Clock>,
    config: CommunityServiceConfig,
}

impl CommunityService {
    pub fn new(
        repo: Arc<dyn CommunityRepository>,
        tokens: Arc<dyn TokenService>,
        clock: Arc<dyn Clock>,
        config: CommunityServiceConfig,
    ) -> Self {
        Self {
            repo,
            tokens,
            clock,
            config,
        }
    }

    pub async fn create(
        &self,
        owner: UserId,
        name: &str,
        description: &str,
    ) -> Result<Community, AppError> {
        let name = name.trim();
        let description = description.trim();
        if !(2..=80).contains(&name.chars().count()) || description.chars().count() > 500 {
            return Err(AppError::InvalidConversationRequest);
        }
        self.repo
            .create(owner, name, description, self.clock.now())
            .await
    }

    pub async fn list(&self, caller: UserId) -> Result<Vec<Community>, AppError> {
        self.repo.list_for_member(caller).await
    }

    pub async fn update_settings(
        &self,
        caller: UserId,
        community: CommunityId,
        name: &str,
        description: &str,
    ) -> Result<Community, AppError> {
        let name = name.trim();
        let description = description.trim();
        if !(2..=80).contains(&name.chars().count()) || description.chars().count() > 500 {
            return Err(AppError::InvalidConversationRequest);
        }
        self.repo
            .update_settings(caller, community, name, description)
            .await
    }

    pub async fn transfer_ownership(
        &self,
        caller: UserId,
        community: CommunityId,
        new_owner: UserId,
        confirmed: bool,
    ) -> Result<(), AppError> {
        if !confirmed || caller == new_owner {
            return Err(AppError::InvalidConversationRequest);
        }
        self.repo
            .transfer_ownership(caller, community, new_owner)
            .await
    }

    pub async fn delete_community(
        &self,
        caller: UserId,
        community: CommunityId,
        confirmed: bool,
    ) -> Result<(), AppError> {
        if !confirmed {
            return Err(AppError::InvalidConversationRequest);
        }
        self.repo.delete_community(caller, community).await
    }

    pub async fn members(
        &self,
        caller: UserId,
        community: CommunityId,
    ) -> Result<Vec<User>, AppError> {
        self.repo.members(caller, community).await
    }

    pub async fn leave(&self, caller: UserId, community: CommunityId) -> Result<(), AppError> {
        self.repo.leave(caller, community).await
    }

    pub async fn remove_member(
        &self,
        caller: UserId,
        community: CommunityId,
        member: UserId,
    ) -> Result<(), AppError> {
        if caller == member {
            return Err(AppError::NotFound);
        }
        self.repo.remove_member(caller, community, member).await
    }

    pub async fn create_invite(
        &self,
        caller: UserId,
        community: CommunityId,
        ttl_seconds: Option<i64>,
        max_uses: Option<i32>,
    ) -> Result<String, AppError> {
        if max_uses.is_some_and(|limit| limit < 1) {
            return Err(AppError::InvalidConversationRequest);
        }
        let expires_at = match ttl_seconds {
            Some(seconds) if seconds <= 0 => return Err(AppError::InvalidConversationRequest),
            Some(seconds) => {
                let ttl = Duration::seconds(seconds).min(self.config.max_invite_ttl);
                Some(self.clock.now() + ttl)
            }
            None => None,
        };
        let token = self.tokens.generate();
        self.repo
            .create_invite(
                caller,
                community,
                token.digest,
                self.clock.now(),
                expires_at,
                max_uses,
            )
            .await?;
        Ok(token.raw)
    }

    pub async fn join(&self, caller: UserId, invite: &str) -> Result<Community, AppError> {
        if invite.len() < 32 || invite.len() > 128 {
            return Err(AppError::NotFound);
        }
        self.repo
            .join(caller, self.tokens.digest(invite), self.clock.now())
            .await
    }

    pub async fn revoke_invite(
        &self,
        caller: UserId,
        community: CommunityId,
        invite: &str,
    ) -> Result<(), AppError> {
        if invite.len() < 32 || invite.len() > 128 {
            return Err(AppError::NotFound);
        }
        self.repo
            .revoke_invite(caller, community, self.tokens.digest(invite))
            .await
    }

    pub async fn revoke_invite_by_id(
        &self,
        caller: UserId,
        community: CommunityId,
        invite: rustchat_domain::identity::InviteId,
    ) -> Result<(), AppError> {
        self.repo
            .revoke_invite_by_id(caller, community, invite)
            .await
    }

    /// Audit history of one community, newest first, for callers allowed to manage it.
    pub async fn list_audit(
        &self,
        caller: UserId,
        community: CommunityId,
        before: Option<rustchat_domain::identity::AuditEventId>,
        limit: usize,
    ) -> Result<crate::ports::AuditPage, AppError> {
        self.repo
            .list_audit(caller, community, before, limit.clamp(1, 100))
            .await
    }

    pub async fn list_invites(
        &self,
        caller: UserId,
        community: CommunityId,
    ) -> Result<Vec<rustchat_domain::community::Invite>, AppError> {
        self.repo.list_invites(caller, community).await
    }

    pub async fn create_channel(
        &self,
        caller: UserId,
        community: CommunityId,
        name: &str,
        description: &str,
        category: Option<CategoryId>,
        read_only: bool,
    ) -> Result<Channel, AppError> {
        let name = name.trim().to_ascii_lowercase();
        let description = description.trim();
        if !(1..=64).contains(&name.len())
            || !name.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
            })
            || description.chars().count() > 280
        {
            return Err(AppError::InvalidConversationRequest);
        }
        self.repo
            .create_channel(
                caller,
                community,
                &name,
                description,
                category,
                read_only,
                self.clock.now(),
            )
            .await
    }

    pub async fn channels(
        &self,
        caller: UserId,
        community: CommunityId,
    ) -> Result<Vec<Channel>, AppError> {
        self.repo.list_channels(caller, community).await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_channel(
        &self,
        caller: UserId,
        channel: ChannelId,
        name: &str,
        description: &str,
        category: Option<Option<CategoryId>>,
        read_only: bool,
        position: i32,
    ) -> Result<Channel, AppError> {
        let name = name.trim().to_ascii_lowercase();
        let description = description.trim();
        if !(1..=64).contains(&name.len())
            || !name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_'))
            || description.chars().count() > 280
            || position < 0
        {
            return Err(AppError::InvalidConversationRequest);
        }
        self.repo
            .update_channel(
                caller,
                channel,
                &name,
                description,
                category,
                read_only,
                position,
            )
            .await
    }

    pub async fn create_category(
        &self,
        caller: UserId,
        community: CommunityId,
        name: &str,
    ) -> Result<Category, AppError> {
        let name = name.trim();
        if !(1..=64).contains(&name.chars().count()) {
            return Err(AppError::InvalidConversationRequest);
        }
        self.repo.create_category(caller, community, name).await
    }

    pub async fn categories(
        &self,
        caller: UserId,
        community: CommunityId,
    ) -> Result<Vec<Category>, AppError> {
        self.repo.list_categories(caller, community).await
    }

    pub async fn update_category(
        &self,
        caller: UserId,
        category: CategoryId,
        name: &str,
        position: i32,
    ) -> Result<Category, AppError> {
        let name = name.trim();
        if !(1..=64).contains(&name.chars().count()) || position < 0 {
            return Err(AppError::InvalidConversationRequest);
        }
        self.repo
            .update_category(caller, category, name, position)
            .await
    }

    pub async fn delete_category(
        &self,
        caller: UserId,
        category: CategoryId,
    ) -> Result<(), AppError> {
        self.repo.delete_category(caller, category).await
    }

    pub async fn delete_channel(&self, caller: UserId, channel: ChannelId) -> Result<(), AppError> {
        self.repo.delete_channel(caller, channel).await
    }

    pub async fn create_role(
        &self,
        caller: UserId,
        community: CommunityId,
        name: &str,
        permissions: u64,
    ) -> Result<Role, AppError> {
        let name = name.trim();
        if !(2..=32).contains(&name.chars().count()) || permissions & !ALL_PERMISSIONS != 0 {
            return Err(AppError::InvalidConversationRequest);
        }
        self.repo
            .create_role(caller, community, name, permissions)
            .await
    }

    pub async fn roles(
        &self,
        caller: UserId,
        community: CommunityId,
    ) -> Result<Vec<Role>, AppError> {
        self.repo.list_roles(caller, community).await
    }

    pub async fn update_role(
        &self,
        caller: UserId,
        community: CommunityId,
        role: RoleId,
        name: &str,
        permissions: u64,
        position: i32,
    ) -> Result<Role, AppError> {
        let name = name.trim();
        if !(2..=32).contains(&name.chars().count())
            || permissions & !ALL_PERMISSIONS != 0
            || position < 1
        {
            return Err(AppError::InvalidConversationRequest);
        }
        self.repo
            .update_role(caller, community, role, name, permissions, position)
            .await
    }

    pub async fn delete_role(
        &self,
        caller: UserId,
        community: CommunityId,
        role: RoleId,
    ) -> Result<(), AppError> {
        self.repo.delete_role(caller, community, role).await
    }

    pub async fn assign_role(
        &self,
        caller: UserId,
        community: CommunityId,
        member: UserId,
        role: RoleId,
    ) -> Result<(), AppError> {
        self.repo.assign_role(caller, community, member, role).await
    }

    pub async fn remove_role(
        &self,
        caller: UserId,
        community: CommunityId,
        member: UserId,
        role: RoleId,
    ) -> Result<(), AppError> {
        self.repo.remove_role(caller, community, member, role).await
    }

    pub async fn set_channel_override(
        &self,
        caller: UserId,
        channel: ChannelId,
        role: RoleId,
        allow: u64,
        deny: u64,
    ) -> Result<rustchat_domain::identity::CommunityId, AppError> {
        if (allow | deny) & !ALL_PERMISSIONS != 0 || allow & deny != 0 {
            return Err(AppError::InvalidConversationRequest);
        }
        self.repo
            .set_channel_override(caller, channel, role, allow, deny)
            .await
    }
}
