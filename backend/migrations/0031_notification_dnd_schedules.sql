CREATE TABLE user_notification_dnd_schedules (
    user_id uuid PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    enabled boolean NOT NULL DEFAULT false,
    start_time text NOT NULL DEFAULT '22:00' CHECK (start_time ~ '^([01][0-9]|2[0-3]):[0-5][0-9]$'),
    end_time text NOT NULL DEFAULT '08:00' CHECK (end_time ~ '^([01][0-9]|2[0-3]):[0-5][0-9]$'),
    timezone text NOT NULL DEFAULT 'UTC',
    weekdays smallint[] NOT NULL DEFAULT ARRAY[1,2,3,4,5,6,7]::smallint[],
    updated_at timestamptz NOT NULL DEFAULT now(),
    CHECK (cardinality(weekdays) BETWEEN 1 AND 7),
    CHECK (weekdays <@ ARRAY[1,2,3,4,5,6,7]::smallint[])
);
