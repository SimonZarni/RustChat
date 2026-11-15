ALTER TABLE conversations
    ADD COLUMN last_sequence bigint NOT NULL DEFAULT 0 CHECK (last_sequence >= 0);

CREATE TABLE messages (
    id uuid PRIMARY KEY,
    conversation_id uuid NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    sequence bigint NOT NULL CHECK (sequence > 0),
    sender_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    client_message_id uuid NOT NULL,
    content text NOT NULL CHECK (char_length(content) BETWEEN 1 AND 4000 AND octet_length(content) <= 16000),
    request_fingerprint bytea NOT NULL CHECK (octet_length(request_fingerprint) = 32),
    created_at timestamptz NOT NULL,
    CONSTRAINT messages_conversation_sequence_key UNIQUE (conversation_id, sequence),
    CONSTRAINT messages_idempotency_key UNIQUE (conversation_id, sender_id, client_message_id)
);

CREATE INDEX messages_conversation_sequence_idx
    ON messages (conversation_id, sequence DESC);

CREATE TABLE outbox_events (
    id uuid PRIMARY KEY,
    conversation_id uuid NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    sequence bigint NOT NULL CHECK (sequence > 0),
    event_type text NOT NULL,
    payload jsonb NOT NULL,
    created_at timestamptz NOT NULL,
    published_at timestamptz,
    CONSTRAINT outbox_events_conversation_sequence_type_key
        UNIQUE (conversation_id, sequence, event_type)
);

CREATE INDEX outbox_events_pending_idx
    ON outbox_events (created_at, id) WHERE published_at IS NULL;
