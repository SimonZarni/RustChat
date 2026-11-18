CREATE TABLE conversation_notification_preferences (
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    conversation_id uuid NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    messages_enabled boolean NOT NULL DEFAULT true,
    mentions_enabled boolean NOT NULL DEFAULT true,
    muted boolean NOT NULL DEFAULT false,
    muted_until timestamptz,
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, conversation_id),
    CHECK (NOT (muted AND muted_until IS NOT NULL))
);
