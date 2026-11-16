-- Idle timeout and token rotation for sessions.
--
-- last_seen_at records recent activity (updated at most every few minutes),
-- token_rotated_at when the cookie token was last replaced, and the previous
-- token digest stays valid for a short grace period so that parallel requests
-- already in flight with the old cookie are not signed out by a rotation.
-- Existing sessions are treated as active at migration time.
ALTER TABLE sessions
    ADD COLUMN last_seen_at timestamptz NOT NULL DEFAULT now(),
    ADD COLUMN token_rotated_at timestamptz NOT NULL DEFAULT now(),
    ADD COLUMN previous_token_digest bytea
        CONSTRAINT sessions_previous_token_digest_len CHECK (octet_length(previous_token_digest) = 32),
    ADD COLUMN previous_token_valid_until timestamptz,
    ADD CONSTRAINT sessions_previous_token_pair CHECK (
        (previous_token_digest IS NULL) = (previous_token_valid_until IS NULL)
    );

CREATE UNIQUE INDEX sessions_previous_token_digest_key
    ON sessions (previous_token_digest)
    WHERE previous_token_digest IS NOT NULL;
