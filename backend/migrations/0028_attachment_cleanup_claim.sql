ALTER TABLE message_attachments DROP CONSTRAINT message_attachments_state_check;
ALTER TABLE message_attachments ADD CONSTRAINT message_attachments_state_check
    CHECK (state IN ('pending', 'ready', 'deleting'));
