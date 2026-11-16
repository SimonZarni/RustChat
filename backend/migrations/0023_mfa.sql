-- Time-based one-time-password (TOTP) multi-factor authentication.
--
-- The TOTP secret must be recoverable to verify codes, so it is stored
-- encrypted (AES-256-GCM, authenticated against the owning user id) with a
-- server key that is never stored in the database. A row is "pending" until
-- the owner proves possession by entering a valid code; only confirmed rows
-- affect login.
CREATE TABLE mfa_totp (
    user_id           uuid PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    secret_ciphertext bytea       NOT NULL,
    secret_nonce      bytea       NOT NULL CHECK (octet_length(secret_nonce) = 12),
    created_at        timestamptz NOT NULL,
    confirmed_at      timestamptz,
    -- Highest accepted 30-second time step; a step is accepted at most once.
    last_used_step    bigint CHECK (last_used_step IS NULL OR last_used_step >= 0)
);

-- Single-use recovery codes, stored only as SHA-256 digests.
CREATE TABLE mfa_recovery_codes (
    id          uuid PRIMARY KEY,
    user_id     uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    code_digest bytea       NOT NULL UNIQUE CHECK (octet_length(code_digest) = 32),
    created_at  timestamptz NOT NULL,
    used_at     timestamptz
);
CREATE INDEX mfa_recovery_codes_user_idx ON mfa_recovery_codes (user_id) WHERE used_at IS NULL;

-- A password-verified login waiting for its second factor. The bearer token is
-- stored as a digest, expires within minutes and allows a few code attempts.
CREATE TABLE mfa_challenges (
    id           uuid PRIMARY KEY,
    user_id      uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    token_digest bytea       NOT NULL UNIQUE CHECK (octet_length(token_digest) = 32),
    created_at   timestamptz NOT NULL,
    expires_at   timestamptz NOT NULL,
    attempts     integer     NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    CHECK (expires_at > created_at)
);
CREATE INDEX mfa_challenges_expiry_idx ON mfa_challenges (expires_at);
