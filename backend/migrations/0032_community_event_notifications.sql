ALTER TABLE notifications ADD COLUMN community_id uuid REFERENCES communities(id) ON DELETE CASCADE;
ALTER TABLE notifications ADD COLUMN event_type text;
ALTER TABLE notifications DROP CONSTRAINT notifications_kind_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_kind_check CHECK (kind IN ('direct_message','mention','friend_request','community_event'));
ALTER TABLE notifications DROP CONSTRAINT notifications_source_shape;
ALTER TABLE notifications ADD CONSTRAINT notifications_source_shape CHECK (
    (kind = 'friend_request' AND conversation_id IS NULL AND message_id IS NULL AND community_id IS NULL AND event_type IS NULL)
    OR (kind IN ('direct_message','mention') AND conversation_id IS NOT NULL AND message_id IS NOT NULL AND community_id IS NULL AND event_type IS NULL)
    OR (kind = 'community_event' AND conversation_id IS NULL AND message_id IS NULL AND community_id IS NOT NULL AND event_type IN ('member_joined','channel_created'))
);
