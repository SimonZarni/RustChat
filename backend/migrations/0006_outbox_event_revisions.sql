-- A message can be edited repeatedly, so an event type may recur at the
-- same conversation sequence. Event UUIDs provide idempotent event identity.
ALTER TABLE outbox_events
    DROP CONSTRAINT outbox_events_conversation_sequence_type_key;
