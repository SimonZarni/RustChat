ALTER TABLE messages
    ADD COLUMN reply_to_message_id uuid;

ALTER TABLE messages
    ADD CONSTRAINT messages_reply_target_same_conversation_fk
    FOREIGN KEY (reply_to_message_id, conversation_id)
    REFERENCES messages (id, conversation_id)
    ON DELETE SET NULL (reply_to_message_id);

CREATE INDEX messages_reply_to_message_idx
    ON messages (reply_to_message_id)
    WHERE reply_to_message_id IS NOT NULL;
