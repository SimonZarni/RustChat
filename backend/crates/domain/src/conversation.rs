//! Conversation entities and membership.

use time::OffsetDateTime;

use crate::identity::{ConversationId, UserId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationKind {
    Direct,
    Group,
}

/// Maximum length of a group name, in characters.
pub const CONVERSATION_NAME_MAX_CHARS: usize = 80;

/// A validated group name: trimmed, 1-80 characters, no control characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationName(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ConversationNameError {
    #[error("group name cannot be empty")]
    Empty,
    #[error("group name must be at most 80 characters")]
    TooLong,
    #[error("group name cannot contain control characters")]
    ControlCharacters,
}

impl ConversationName {
    pub fn parse(value: &str) -> Result<Self, ConversationNameError> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(ConversationNameError::Empty);
        }
        if trimmed.chars().any(char::is_control) {
            return Err(ConversationNameError::ControlCharacters);
        }
        if trimmed.chars().count() > CONVERSATION_NAME_MAX_CHARS {
            return Err(ConversationNameError::TooLong);
        }
        Ok(Self(trimmed.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversation {
    pub id: ConversationId,
    pub kind: ConversationKind,
    /// Only group conversations can be named.
    pub name: Option<ConversationName>,
    pub created_by: UserId,
    pub created_at: OffsetDateTime,
    pub members: Vec<UserId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationPage {
    pub conversations: Vec<Conversation>,
    pub next_cursor: Option<ConversationId>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_trimmed_and_bounded() {
        assert_eq!(
            ConversationName::parse("  Weekend plans \t")
                .unwrap()
                .as_str(),
            "Weekend plans"
        );
        assert_eq!(
            ConversationName::parse("   "),
            Err(ConversationNameError::Empty)
        );
        assert_eq!(
            ConversationName::parse(""),
            Err(ConversationNameError::Empty)
        );
        assert_eq!(
            ConversationName::parse(&"x".repeat(CONVERSATION_NAME_MAX_CHARS + 1)),
            Err(ConversationNameError::TooLong)
        );
        assert!(ConversationName::parse(&"x".repeat(CONVERSATION_NAME_MAX_CHARS)).is_ok());
        // The limit counts characters, not bytes.
        assert!(ConversationName::parse(&"é".repeat(CONVERSATION_NAME_MAX_CHARS)).is_ok());
    }

    #[test]
    fn control_characters_are_rejected() {
        assert_eq!(
            ConversationName::parse("a\nb"),
            Err(ConversationNameError::ControlCharacters)
        );
        assert_eq!(
            ConversationName::parse("a\u{0}b"),
            Err(ConversationNameError::ControlCharacters)
        );
    }
}
