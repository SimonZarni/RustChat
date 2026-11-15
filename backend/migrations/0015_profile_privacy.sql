ALTER TABLE user_profiles
    ADD COLUMN discoverable boolean NOT NULL DEFAULT true,
    ADD COLUMN profile_visibility text NOT NULL DEFAULT 'everyone',
    ADD CONSTRAINT user_profiles_visibility_check
        CHECK (profile_visibility IN ('everyone', 'conversation_members'));
