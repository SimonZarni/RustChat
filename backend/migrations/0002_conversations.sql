CREATE TABLE conversations (
    id uuid PRIMARY KEY,
    kind text NOT NULL CHECK (kind IN ('direct', 'group')),
    created_by uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL,
    direct_user_low uuid REFERENCES users(id) ON DELETE CASCADE,
    direct_user_high uuid REFERENCES users(id) ON DELETE CASCADE,
    CONSTRAINT conversations_kind_pair_check CHECK (
        (kind = 'direct' AND direct_user_low IS NOT NULL AND direct_user_high IS NOT NULL
            AND direct_user_low < direct_user_high)
        OR (kind = 'group' AND direct_user_low IS NULL AND direct_user_high IS NULL)
    ),
    CONSTRAINT conversations_direct_pair_key UNIQUE (direct_user_low, direct_user_high)
);

CREATE TABLE conversation_members (
    conversation_id uuid NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    joined_at timestamptz NOT NULL,
    PRIMARY KEY (conversation_id, user_id)
);

CREATE INDEX conversation_members_user_conversation_idx
    ON conversation_members (user_id, conversation_id);
