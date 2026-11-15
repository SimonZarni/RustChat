#!/bin/sh
set -eu
case "${RUSTCHAT_APP_DB_PASSWORD:?}" in *[!A-Za-z0-9_-]*|'') echo "APP_DB_PASSWORD must use URL-safe letters, digits, _ or -" >&2; exit 1;; esac
case "${RUSTCHAT_MIGRATOR_DB_PASSWORD:?}" in *[!A-Za-z0-9_-]*|'') echo "MIGRATOR_DB_PASSWORD must use URL-safe letters, digits, _ or -" >&2; exit 1;; esac
psql --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" \
    --set=app_password="$RUSTCHAT_APP_DB_PASSWORD" \
    --set=migrator_password="$RUSTCHAT_MIGRATOR_DB_PASSWORD" <<'SQL'
CREATE ROLE rustchat_migrator LOGIN PASSWORD :'migrator_password';
CREATE ROLE rustchat_app LOGIN PASSWORD :'app_password';
GRANT CONNECT ON DATABASE rustchat TO rustchat_migrator, rustchat_app;
GRANT USAGE, CREATE ON SCHEMA public TO rustchat_migrator;
GRANT USAGE ON SCHEMA public TO rustchat_app;
ALTER DEFAULT PRIVILEGES FOR ROLE rustchat_migrator IN SCHEMA public
    GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO rustchat_app;
ALTER DEFAULT PRIVILEGES FOR ROLE rustchat_migrator IN SCHEMA public
    GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO rustchat_app;
SQL
