-- Support for running several server instances against one database.
--
-- Outbox rows are claimed with a short lease so that exactly one instance
-- dispatches each event. If that instance dies before marking the row
-- published, the lease expires and another instance picks it up (at-least-once).
ALTER TABLE outbox_events ADD COLUMN claimed_until timestamptz;

-- Which instance holds a live WebSocket for which user. A user is online while
-- any instance has refreshed a row for them recently; rows of a crashed
-- instance simply stop being refreshed and expire.
CREATE TABLE presence_connections (
    user_id      uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    instance_id  uuid        NOT NULL,
    connected_at timestamptz NOT NULL,
    refreshed_at timestamptz NOT NULL,
    PRIMARY KEY (user_id, instance_id)
);
CREATE INDEX presence_connections_refreshed_idx ON presence_connections (refreshed_at);
