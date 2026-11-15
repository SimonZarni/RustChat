CREATE TABLE community_categories (
    id uuid PRIMARY KEY,
    community_id uuid NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 64),
    position integer NOT NULL CHECK (position >= 0),
    CONSTRAINT community_categories_name_key UNIQUE (community_id, name),
    CONSTRAINT community_categories_position_key UNIQUE (community_id, position)
);

ALTER TABLE community_channels
    ADD COLUMN category_id uuid REFERENCES community_categories(id) ON DELETE SET NULL;

CREATE INDEX community_channels_category_order_idx
    ON community_channels (category_id, position, id);
