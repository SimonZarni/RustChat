-- Account lifecycle states. Account deletion is a retained tombstone so
-- message and audit foreign keys preserve historical records.
ALTER TABLE users
    ADD COLUMN deactivated_at timestamptz,
    ADD COLUMN deleted_at timestamptz,
    ADD CONSTRAINT users_deleted_is_deactivated
        CHECK (deleted_at IS NULL OR deactivated_at IS NOT NULL);
