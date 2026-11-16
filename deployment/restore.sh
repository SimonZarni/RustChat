#!/bin/sh
# Restore a dump made by backup.sh or backup.ps1 into the Compose stack's database
# (the POSIX counterpart of restore.ps1). This REPLACES the database contents.
#
#   deployment/restore.sh <backup-file>
#
# Run deployment/verify-backup.sh on the file first; it restores into a scratch
# database and does not touch the live one.
#
# Environment: RUSTCHAT_ENV_FILE (default deployment/.env), RUSTCHAT_PROJECT (default rustchat).
set -eu

if [ $# -ne 1 ] || [ ! -f "$1" ]; then
    echo "usage: $0 <backup-file>" >&2
    exit 2
fi
file="$1"
env_file="${RUSTCHAT_ENV_FILE:-deployment/.env}"
project="${RUSTCHAT_PROJECT:-rustchat}"
compose() {
    docker compose --project-name "$project" --env-file "$env_file" -f deployment/compose.yml "$@"
}

echo "WARNING: this replaces the RustChat database contents. Verify that this is the intended backup." >&2
compose stop server
compose exec -T db pg_restore -U rustchat -d rustchat --clean --if-exists --no-owner < "$file"
compose start server
echo "Restored $file. Check /readyz, sign in, and open a conversation."
