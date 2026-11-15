CREATE TABLE message_mentions (
    message_id uuid NOT NULL,
    conversation_id uuid NOT NULL,
    user_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (message_id, user_id),
    CONSTRAINT message_mentions_message_fk
        FOREIGN KEY (message_id, conversation_id)
        REFERENCES messages (id, conversation_id) ON DELETE CASCADE,
    CONSTRAINT message_mentions_member_fk
        FOREIGN KEY (conversation_id, user_id)
        REFERENCES conversation_members (conversation_id, user_id) ON DELETE CASCADE
);

CREATE INDEX message_mentions_user_idx
    ON message_mentions (user_id, message_id);
