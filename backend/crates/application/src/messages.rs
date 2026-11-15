//! Message send and history use cases.

use std::sync::Arc;

use rustchat_domain::{
    identity::{ConversationId, MessageId, UserId},
    message::{
        DeliveryCursor, Message, MessageContent, MessagePage, MessageReaction, ReactionEmoji,
        ReactionMutation, ReadCursor,
    },
};

use crate::{error::AppError, ports::MessageRepository};

#[derive(Debug, Clone, Copy)]
pub struct MessageConfig {
    pub max_page_size: usize,
}

pub struct MessageService {
    messages: Arc<dyn MessageRepository>,
    config: MessageConfig,
}

impl MessageService {
    pub fn new(messages: Arc<dyn MessageRepository>, config: MessageConfig) -> Self {
        Self { messages, config }
    }

    pub async fn send(
        &self,
        caller: UserId,
        conversation: ConversationId,
        client_message_id: MessageId,
        reply_to_message_id: Option<MessageId>,
        content: String,
    ) -> Result<Message, AppError> {
        let content = MessageContent::parse(content)?;
        self.messages
            .send(
                caller,
                conversation,
                client_message_id,
                reply_to_message_id,
                content,
            )
            .await
    }

    pub async fn list(
        &self,
        caller: UserId,
        conversation: ConversationId,
        before_sequence: Option<i64>,
        after_sequence: Option<i64>,
        limit: usize,
    ) -> Result<MessagePage, AppError> {
        self.messages
            .list_for_member(
                caller,
                conversation,
                before_sequence,
                after_sequence,
                limit.clamp(1, self.config.max_page_size),
            )
            .await
    }

    pub async fn edit(
        &self,
        caller: UserId,
        conversation: ConversationId,
        message: MessageId,
        content: String,
    ) -> Result<Message, AppError> {
        self.messages
            .edit(
                caller,
                conversation,
                message,
                MessageContent::parse(content)?,
            )
            .await
    }

    pub async fn delete(
        &self,
        caller: UserId,
        conversation: ConversationId,
        message: MessageId,
    ) -> Result<Message, AppError> {
        self.messages.delete(caller, conversation, message).await
    }

    pub async fn advance_read_cursor(
        &self,
        caller: UserId,
        conversation: ConversationId,
        sequence: i64,
    ) -> Result<ReadCursor, AppError> {
        if sequence < 0 {
            return Err(AppError::InvalidReadCursor);
        }
        self.messages
            .advance_read_cursor(
                caller,
                conversation,
                sequence,
                time::OffsetDateTime::now_utc(),
            )
            .await
    }

    pub async fn read_cursors(
        &self,
        caller: UserId,
        conversation: ConversationId,
    ) -> Result<Vec<ReadCursor>, AppError> {
        self.messages
            .read_cursors_for_member(caller, conversation)
            .await
    }

    pub async fn list_reactions(
        &self,
        caller: UserId,
        conversation: ConversationId,
        messages: &[MessageId],
    ) -> Result<Vec<MessageReaction>, AppError> {
        if messages.len() > 100 {
            return Err(AppError::InvalidReactionRequest);
        }
        self.messages
            .list_reactions(caller, conversation, messages)
            .await
    }

    pub async fn set_reaction(
        &self,
        caller: UserId,
        conversation: ConversationId,
        message: MessageId,
        emoji: String,
        added: bool,
    ) -> Result<ReactionMutation, AppError> {
        let emoji = ReactionEmoji::parse(emoji)?;
        self.messages
            .set_reaction(
                caller,
                conversation,
                message,
                emoji,
                added,
                time::OffsetDateTime::now_utc(),
            )
            .await
    }

    pub async fn advance_delivery_cursor(
        &self,
        caller: UserId,
        conversation: ConversationId,
        sequence: i64,
    ) -> Result<DeliveryCursor, AppError> {
        if sequence < 0 {
            return Err(AppError::InvalidDeliveryCursor);
        }
        self.messages
            .advance_delivery_cursor(
                caller,
                conversation,
                sequence,
                time::OffsetDateTime::now_utc(),
            )
            .await
    }

    pub async fn delivery_cursors(
        &self,
        caller: UserId,
        conversation: ConversationId,
    ) -> Result<Vec<DeliveryCursor>, AppError> {
        self.messages
            .delivery_cursors_for_member(caller, conversation)
            .await
    }
}
