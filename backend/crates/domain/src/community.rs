//! Communities, text channels and their permission data.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::identity::{CategoryId, ChannelId, CommunityId, InviteId, RoleId, UserId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Community {
    pub id: CommunityId,
    pub owner_id: UserId,
    pub name: String,
    pub description: String,
    pub created_at: OffsetDateTime,
    pub member_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel {
    pub id: ChannelId,
    pub community_id: CommunityId,
    pub conversation_id: crate::identity::ConversationId,
    pub category_id: Option<CategoryId>,
    pub channel_type: ChannelType,
    pub name: String,
    pub description: String,
    pub position: i32,
    pub read_only: bool,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelType {
    Text,
    Voice,
}

impl Default for ChannelType {
    fn default() -> Self {
        Self::Text
    }
}

impl ChannelType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Voice => "voice",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Category {
    pub id: CategoryId,
    pub community_id: CommunityId,
    pub name: String,
    pub position: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Role {
    pub id: RoleId,
    pub community_id: CommunityId,
    pub name: String,
    pub position: i32,
    pub permissions: u64,
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invite {
    pub id: InviteId,
    pub community_id: CommunityId,
    pub created_by: UserId,
    pub created_at: OffsetDateTime,
    pub expires_at: Option<OffsetDateTime>,
    pub max_uses: Option<i32>,
    pub use_count: i32,
    pub revoked_at: Option<OffsetDateTime>,
}
