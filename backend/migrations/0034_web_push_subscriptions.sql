CREATE TABLE web_push_subscriptions (
    id uuid PRIMARY KEY,
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    endpoint text NOT NULL UNIQUE CHECK (char_length(endpoint) BETWEEN 24 AND 2048),
    p256dh text NOT NULL CHECK (char_length(p256dh) BETWEEN 40 AND 128),
    auth_secret text NOT NULL CHECK (char_length(auth_secret) BETWEEN 16 AND 64),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX web_push_subscriptions_user_idx ON web_push_subscriptions(user_id);
