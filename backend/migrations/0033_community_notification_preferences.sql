ALTER TABLE notifications DROP CONSTRAINT notifications_source_shape;
ALTER TABLE notifications ADD CONSTRAINT notifications_source_shape CHECK (
    (kind = 'friend_request' AND conversation_id IS NULL AND message_id IS NULL AND community_id IS NULL AND event_type IS NULL)
    OR (kind IN ('direct_message','mention') AND conversation_id IS NOT NULL AND message_id IS NOT NULL AND community_id IS NULL AND event_type IS NULL)
    OR (kind = 'community_event' AND conversation_id IS NULL AND message_id IS NULL AND community_id IS NOT NULL AND event_type IN ('member_joined','member_left','member_removed','channel_created','channel_updated','channel_deleted'))
);

CREATE TABLE community_notification_preferences (
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    community_id uuid NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
    member_events_enabled boolean NOT NULL DEFAULT true,
    channel_events_enabled boolean NOT NULL DEFAULT true,
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, community_id)
);
