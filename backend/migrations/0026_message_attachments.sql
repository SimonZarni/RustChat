CREATE TABLE message_attachments (
    id uuid PRIMARY KEY,
    conversation_id uuid NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    owner_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    message_id uuid NULL,
    position smallint NULL,
    storage_key text NOT NULL UNIQUE,
    upload_key text NOT NULL UNIQUE,
    file_name text NOT NULL,
    media_type text NOT NULL,
    byte_size bigint NOT NULL CHECK (byte_size > 0),
    state text NOT NULL CHECK (state IN ('pending', 'ready')),
    created_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL,
    FOREIGN KEY (message_id) REFERENCES messages(id) ON DELETE SET NULL
);

CREATE INDEX message_attachments_owner_quota_idx
    ON message_attachments (owner_id, created_at DESC)
    WHERE state = 'ready';
CREATE INDEX message_attachments_pending_expiry_idx
    ON message_attachments (expires_at)
    WHERE state = 'pending';
CREATE INDEX message_attachments_message_idx
    ON message_attachments (message_id, created_at)
    WHERE message_id IS NOT NULL;

ALTER TABLE message_attachments
    ADD CONSTRAINT message_attachments_message_state_check
    CHECK (message_id IS NULL OR (position IS NOT NULL AND position BETWEEN 0 AND 9 AND state = 'ready'));
CREATE UNIQUE INDEX message_attachments_message_position_idx
    ON message_attachments (message_id, position)
    WHERE message_id IS NOT NULL;

ALTER TABLE messages DROP CONSTRAINT messages_content_check;
ALTER TABLE messages ADD CONSTRAINT messages_content_check
    CHECK (char_length(content) BETWEEN 0 AND 4000 AND octet_length(content) <= 16000);
