-- Optional display name for group conversations. Direct conversations are named
-- after the other person in the client, and community channels use their channel
-- name, so only groups may carry one. The database enforces the same bounds as the
-- application (1-80 characters, no surrounding whitespace, no control characters).
ALTER TABLE conversations
    ADD COLUMN name text,
    ADD CONSTRAINT conversations_name_check CHECK (
        name IS NULL
        OR (
            kind = 'group'
            AND char_length(name) BETWEEN 1 AND 80
            AND name = btrim(name)
            AND name !~ '[[:cntrl:]]'
        )
    );
