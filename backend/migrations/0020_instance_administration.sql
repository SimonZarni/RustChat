ALTER TABLE users ADD COLUMN suspended_at timestamptz;

CREATE TABLE instance_admin_audit_log (
    id uuid PRIMARY KEY,
    actor_user_id uuid REFERENCES users (id) ON DELETE SET NULL,
    target_user_id uuid REFERENCES users (id) ON DELETE SET NULL,
    event_type text NOT NULL CHECK (event_type IN ('user.suspended', 'user.unsuspended')),
    created_at timestamptz NOT NULL,
    details jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE INDEX instance_admin_audit_log_created_idx
    ON instance_admin_audit_log (created_at DESC, id DESC);
