-- Identity: users and sessions.

CREATE TABLE users (
    id            uuid PRIMARY KEY,
    -- Stored normalized (lower-case); uniqueness is therefore case-insensitive.
    username      text        NOT NULL,
    -- PHC string (Argon2id). Never the plaintext password.
    password_hash text        NOT NULL,
    created_at    timestamptz NOT NULL,
    CONSTRAINT users_username_key UNIQUE (username),
    CONSTRAINT users_username_format CHECK (username ~ '^[a-z0-9_]{3,32}$')
);

CREATE TABLE sessions (
    id           uuid PRIMARY KEY,
    user_id      uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- SHA-256 of the secret token held in the client's cookie. The token
    -- itself is never stored, so a database leak does not expose sessions.
    token_digest bytea       NOT NULL,
    created_at   timestamptz NOT NULL,
    expires_at   timestamptz NOT NULL,
    revoked_at   timestamptz,
    CONSTRAINT sessions_token_digest_key UNIQUE (token_digest),
    CONSTRAINT sessions_token_digest_len CHECK (octet_length(token_digest) = 32),
    CONSTRAINT sessions_expiry_after_creation CHECK (expires_at > created_at)
);

-- Listing a user's live sessions.
CREATE INDEX sessions_user_active_idx ON sessions (user_id) WHERE revoked_at IS NULL;
