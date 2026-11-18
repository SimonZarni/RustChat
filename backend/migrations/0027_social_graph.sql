CREATE TABLE friend_requests (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    sender_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    recipient_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT friend_requests_not_self CHECK (sender_id <> recipient_id),
    CONSTRAINT friend_requests_unique_pair UNIQUE (sender_id, recipient_id)
);
CREATE INDEX friend_requests_recipient_created ON friend_requests (recipient_id, created_at DESC);
CREATE INDEX friend_requests_sender_created ON friend_requests (sender_id, created_at DESC);

CREATE TABLE friendships (
    user_low UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    user_high UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_low, user_high),
    CONSTRAINT friendships_canonical_pair CHECK (user_low < user_high)
);

CREATE TABLE user_blocks (
    blocker_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    blocked_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (blocker_id, blocked_id),
    CONSTRAINT user_blocks_not_self CHECK (blocker_id <> blocked_id)
);
CREATE INDEX user_blocks_reverse ON user_blocks (blocked_id, blocker_id);
