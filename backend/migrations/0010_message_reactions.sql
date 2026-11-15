ALTER TABLE messages
    ADD CONSTRAINT messages_id_conversation_key UNIQUE (id, conversation_id);

CREATE TABLE message_reactions (
    conversation_id uuid NOT NULL,
    message_id uuid NOT NULL,
    user_id uuid NOT NULL,
    emoji text NOT NULL CHECK (
        char_length(emoji) BETWEEN 1 AND 16
        AND octet_length(emoji) <= 64
        AND emoji = btrim(emoji)
    ),
    created_at timestamptz NOT NULL,
    PRIMARY KEY (message_id, user_id, emoji),
    FOREIGN KEY (message_id, conversation_id)
        REFERENCES messages(id, conversation_id) ON DELETE CASCADE,
    FOREIGN KEY (conversation_id, user_id)
        REFERENCES conversation_members(conversation_id, user_id) ON DELETE CASCADE
);

CREATE INDEX message_reactions_conversation_message_idx
    ON message_reactions (conversation_id, message_id);
