-- Shared authentication rate-limit state for every server using this database.
CREATE TABLE rate_limit_entries (
    key_digest bytea PRIMARY KEY CHECK (octet_length(key_digest) = 32),
    count bigint NOT NULL CHECK (count > 0),
    expires_at timestamptz NOT NULL
);

CREATE INDEX rate_limit_entries_expiry_idx ON rate_limit_entries (expires_at);
