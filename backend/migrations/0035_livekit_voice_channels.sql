ALTER TABLE community_channels
    ADD COLUMN channel_type text NOT NULL DEFAULT 'text'
        CHECK (channel_type IN ('text', 'voice'));

-- Members can connect to voice rooms by default; channel overrides can deny it.
UPDATE community_roles
SET permissions = permissions | 512
WHERE is_default;

CREATE OR REPLACE FUNCTION community_permissions(request_user uuid, request_community uuid)
RETURNS bigint LANGUAGE sql STABLE AS $$
    SELECT CASE
        WHEN EXISTS (
            SELECT 1 FROM communities c
            WHERE c.id = request_community AND c.owner_id = request_user
        ) THEN 1023::bigint
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
