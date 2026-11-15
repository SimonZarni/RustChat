CREATE TABLE community_audit_log (
    id uuid PRIMARY KEY,
    community_id uuid REFERENCES communities(id) ON DELETE SET NULL,
    actor_id uuid REFERENCES users(id) ON DELETE SET NULL,
    event_type text NOT NULL,
    details jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_at timestamptz NOT NULL
);
CREATE INDEX community_audit_log_community_created_idx
    ON community_audit_log (community_id, created_at DESC);
