CREATE TABLE read_cursors (
    conversation_id uuid NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    sequence bigint NOT NULL DEFAULT 0 CHECK (sequence >= 0),
    updated_at timestamptz NOT NULL,
    PRIMARY KEY (conversation_id, user_id),
    FOREIGN KEY (conversation_id, user_id)
        REFERENCES conversation_members(conversation_id, user_id) ON DELETE CASCADE
);
