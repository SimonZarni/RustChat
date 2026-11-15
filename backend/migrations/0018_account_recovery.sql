-- Optional account recovery email and single-use password reset tokens.
ALTER TABLE users
    ADD COLUMN email text,
    ADD CONSTRAINT users_email_normalized CHECK (email IS NULL OR email = lower(btrim(email)));

CREATE UNIQUE INDEX users_email_key ON users (email) WHERE email IS NOT NULL;

CREATE TABLE account_tokens (
    id uuid PRIMARY KEY,
    user_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    token_digest bytea NOT NULL UNIQUE CHECK (octet_length(token_digest) = 32),
    created_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL,
    used_at timestamptz,
    CHECK (expires_at > created_at)
);

CREATE INDEX account_tokens_user_created_idx
    ON account_tokens (user_id, created_at DESC)
    WHERE used_at IS NULL;
