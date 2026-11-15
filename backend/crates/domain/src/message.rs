//! Persistent text messages.

use time::OffsetDateTime;
use uuid::Uuid;

use crate::identity::{ConversationId, MessageId, UserId};

pub const MESSAGE_MAX_CHARS: usize = 4_000;
pub const MESSAGE_MAX_BYTES: usize = 16_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageContent(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MessageContentError {
    #[error("message content cannot be empty")]
    Empty,
    #[error("message content exceeds the maximum length")]
    TooLong,
}

impl MessageContent {
    pub fn parse(value: String) -> Result<Self, MessageContentError> {
        if value.trim().is_empty() {
            return Err(MessageContentError::Empty);
        }
        if value.chars().count() > MESSAGE_MAX_CHARS || value.len() > MESSAGE_MAX_BYTES {
            return Err(MessageContentError::TooLong);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub id: MessageId,
    pub conversation_id: ConversationId,
    pub sender_id: UserId,
    pub sequence: i64,
    pub client_message_id: Uuid,
    pub reply_to_message_id: Option<MessageId>,
    pub mentioned_user_ids: Vec<UserId>,
    pub content: MessageContent,
    pub created_at: OffsetDateTime,
    pub edited_at: Option<OffsetDateTime>,
    pub deleted_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessagePage {
    pub messages: Vec<Message>,
    pub next_before: Option<i64>,
    pub next_after: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadCursor {
    pub user_id: crate::identity::UserId,
    pub sequence: i64,
    pub updated_at: time::OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryCursor {
    pub user_id: crate::identity::UserId,
    pub sequence: i64,
    pub updated_at: time::OffsetDateTime,
    pub advanced: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageReaction {
    pub message_id: MessageId,
    pub emoji: String,
    pub count: i64,
    pub reacted_by_me: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReactionMutation {
    pub sequence: i64,
    pub changed: bool,
    pub reactions: Vec<MessageReaction>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReactionEmojiError {
    #[error("reaction must contain a visible emoji")]
    Empty,
    #[error("reaction is too long")]
    TooLong,
    #[error("reaction is not supported")]
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReactionEmoji(String);

impl ReactionEmoji {
    pub fn parse(value: String) -> Result<Self, ReactionEmojiError> {
        let trimmed = value.trim();
        if trimmed.is_empty() || trimmed.chars().any(char::is_control) {
            return Err(ReactionEmojiError::Empty);
        }
        if trimmed.chars().count() > 16 || trimmed.len() > 64 {
            return Err(ReactionEmojiError::TooLong);
        }
        if !["👍", "❤️", "😂", "🎉", "😮", "😢"].contains(&trimmed) {
            return Err(ReactionEmojiError::Unsupported);
        }
        Ok(Self(trimmed.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_must_be_nonblank_and_bounded() {
        assert_eq!(
            MessageContent::parse(" \n ".into()),
            Err(MessageContentError::Empty)
        );
        assert_eq!(
            MessageContent::parse("x".repeat(MESSAGE_MAX_CHARS + 1)),
            Err(MessageContentError::TooLong)
        );
        assert_eq!(
            MessageContent::parse("x".repeat(MESSAGE_MAX_CHARS))
                .unwrap()
                .as_str()
                .len(),
            MESSAGE_MAX_CHARS
        );
    }

    #[test]
    fn maximum_unicode_content_fits_the_byte_ceiling() {
        let value = "🦀".repeat(MESSAGE_MAX_CHARS);
        let parsed = MessageContent::parse(value).unwrap();
        assert_eq!(parsed.as_str().len(), MESSAGE_MAX_BYTES);
    }

    #[test]
    fn reaction_emoji_is_trimmed_and_bounded() {
        assert_eq!(ReactionEmoji::parse(" 👍 ".into()).unwrap().as_str(), "👍");
        assert_eq!(
            ReactionEmoji::parse(" \n ".into()),
            Err(ReactionEmojiError::Empty)
        );
        assert_eq!(
            ReactionEmoji::parse("👍".repeat(17)),
            Err(ReactionEmojiError::TooLong)
        );
        assert_eq!(
            ReactionEmoji::parse("not emoji".into()),
            Err(ReactionEmojiError::Unsupported)
        );
    }
}
