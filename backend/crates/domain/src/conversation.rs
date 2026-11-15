//! Conversation entities and membership.

use time::OffsetDateTime;

use crate::identity::{ConversationId, UserId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationKind {
    Direct,
    Group,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversation {
    pub id: ConversationId,
    pub kind: ConversationKind,
    pub created_by: UserId,
    pub created_at: OffsetDateTime,
    pub members: Vec<UserId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationPage {
    pub conversations: Vec<Conversation>,
    pub next_cursor: Option<ConversationId>,
}
