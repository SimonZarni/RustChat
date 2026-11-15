CREATE TABLE communities (
    id uuid PRIMARY KEY,
    owner_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name text NOT NULL CHECK (char_length(name) BETWEEN 2 AND 80),
    description text NOT NULL DEFAULT '' CHECK (char_length(description) <= 500),
    created_at timestamptz NOT NULL
);

CREATE TABLE community_members (
    community_id uuid NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    joined_at timestamptz NOT NULL,
    PRIMARY KEY (community_id, user_id)
);
CREATE INDEX community_members_user_community_idx ON community_members (user_id, community_id);

CREATE TABLE community_roles (
    id uuid PRIMARY KEY,
    community_id uuid NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
    name text NOT NULL CHECK (char_length(name) BETWEEN 2 AND 32),
    position integer NOT NULL CHECK (position >= 0),
    permissions bigint NOT NULL DEFAULT 0 CHECK (permissions >= 0),
    is_default boolean NOT NULL DEFAULT false,
    CONSTRAINT community_roles_name_key UNIQUE (community_id, name),
    CONSTRAINT community_roles_position_key UNIQUE (community_id, position)
);
CREATE UNIQUE INDEX community_roles_one_default_idx ON community_roles (community_id) WHERE is_default;

CREATE TABLE community_member_roles (
    community_id uuid NOT NULL,
    user_id uuid NOT NULL,
    role_id uuid NOT NULL REFERENCES community_roles(id) ON DELETE CASCADE,
    PRIMARY KEY (community_id, user_id, role_id),
    FOREIGN KEY (community_id, user_id) REFERENCES community_members(community_id, user_id) ON DELETE CASCADE
);

CREATE TABLE community_channels (
    id uuid PRIMARY KEY,
    community_id uuid NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
    conversation_id uuid NOT NULL UNIQUE REFERENCES conversations(id) ON DELETE CASCADE,
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 64),
    description text NOT NULL DEFAULT '' CHECK (char_length(description) <= 280),
    position integer NOT NULL CHECK (position >= 0),
    read_only boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL,
    CONSTRAINT community_channels_name_key UNIQUE (community_id, name),
    CONSTRAINT community_channels_position_key UNIQUE (community_id, position)
);

CREATE TABLE channel_role_overrides (
    channel_id uuid NOT NULL REFERENCES community_channels(id) ON DELETE CASCADE,
    role_id uuid NOT NULL REFERENCES community_roles(id) ON DELETE CASCADE,
    allow_permissions bigint NOT NULL DEFAULT 0 CHECK (allow_permissions >= 0),
    deny_permissions bigint NOT NULL DEFAULT 0 CHECK (deny_permissions >= 0),
    PRIMARY KEY (channel_id, role_id),
    CHECK ((allow_permissions & deny_permissions) = 0)
);

CREATE TABLE community_invites (
    id uuid PRIMARY KEY,
    community_id uuid NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
    token_hash bytea NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    created_by uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL,
    expires_at timestamptz,
    max_uses integer CHECK (max_uses IS NULL OR max_uses > 0),
    use_count integer NOT NULL DEFAULT 0 CHECK (use_count >= 0),
    revoked_at timestamptz
);

CREATE FUNCTION community_permissions(request_user uuid, request_community uuid)
RETURNS bigint LANGUAGE sql STABLE AS $$
    SELECT CASE
        WHEN EXISTS (
            SELECT 1 FROM communities c
            WHERE c.id = request_community AND c.owner_id = request_user
        ) THEN 511::bigint
        ELSE COALESCE((
            SELECT bit_or(r.permissions)
            FROM community_members m
            JOIN community_member_roles mr
              ON mr.community_id = m.community_id AND mr.user_id = m.user_id
            JOIN community_roles r ON r.id = mr.role_id
            WHERE m.community_id = request_community AND m.user_id = request_user
        ), 0::bigint)
    END;
$$;

CREATE FUNCTION channel_permissions(request_user uuid, request_channel uuid)
RETURNS bigint LANGUAGE sql STABLE AS $$
    SELECT (community_permissions(request_user, c.community_id) | COALESCE(overrides.allowed, 0::bigint))
        & ~COALESCE(overrides.denied, 0::bigint)
    FROM community_channels c
    LEFT JOIN LATERAL (
        SELECT bit_or(o.allow_permissions) AS allowed, bit_or(o.deny_permissions) AS denied
        FROM channel_role_overrides o
        JOIN community_member_roles mr ON mr.role_id = o.role_id AND mr.community_id = c.community_id AND mr.user_id = request_user
        WHERE o.channel_id = c.id
    ) overrides ON true
    WHERE c.id = request_channel
      AND EXISTS (
          SELECT 1 FROM community_members m
          WHERE m.community_id = c.community_id AND m.user_id = request_user
      );
$$;

CREATE FUNCTION channel_has_permission(request_user uuid, request_channel uuid, required bigint)
RETURNS boolean LANGUAGE sql STABLE AS $$
    SELECT COALESCE((channel_permissions(request_user, request_channel) & required) = required, false);
$$;

CREATE FUNCTION community_has_permission(request_user uuid, request_community uuid, required bigint)
RETURNS boolean LANGUAGE sql STABLE AS $$
    SELECT (community_permissions(request_user, request_community) & required) = required;
$$;
