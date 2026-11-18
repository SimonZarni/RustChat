CREATE TABLE notifications (
    id uuid PRIMARY KEY,
    recipient_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    actor_id uuid NULL REFERENCES users(id) ON DELETE SET NULL,
    kind text NOT NULL CHECK (kind IN ('direct_message', 'mention', 'friend_request')),
    source_id uuid NOT NULL,
    conversation_id uuid NULL REFERENCES conversations(id) ON DELETE SET NULL,
    message_id uuid NULL REFERENCES messages(id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL,
    read_at timestamptz,
    CONSTRAINT notifications_source_shape CHECK (
        (kind = 'friend_request' AND conversation_id IS NULL AND message_id IS NULL)
        OR (kind IN ('direct_message', 'mention') AND conversation_id IS NOT NULL AND message_id IS NOT NULL)
    )
);

CREATE UNIQUE INDEX notifications_source_unique
    ON notifications (recipient_id, kind, source_id);
CREATE INDEX notifications_inbox_idx
    ON notifications (recipient_id, created_at DESC, id DESC);
CREATE INDEX notifications_unread_idx
    ON notifications (recipient_id, created_at DESC)
    WHERE read_at IS NULL;
