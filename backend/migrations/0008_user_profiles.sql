CREATE TABLE user_profiles (
    user_id       uuid PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    display_name  text NOT NULL DEFAULT '',
    bio           text NOT NULL DEFAULT '',
    updated_at    timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT user_profiles_display_name_len CHECK (char_length(display_name) <= 64),
    CONSTRAINT user_profiles_bio_len CHECK (char_length(bio) <= 280)
);

INSERT INTO user_profiles (user_id, display_name)
SELECT id, username FROM users;
