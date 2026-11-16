#!/bin/sh
# Back up the RustChat database to a custom-format dump (the POSIX counterpart of backup.ps1).
#
#   deployment/backup.sh [output-directory]
#
# By default it dumps the Compose stack's database through `docker compose exec`.
# Set PGHOST (and PGPORT, PGUSER, PGPASSWORD, optionally PGDATABASE) to dump a
# PostgreSQL server directly instead. RUSTCHAT_PG_PREFIX can name a command that
# runs the PostgreSQL client tools, for example a container with a matching version.
#
# Environment: RUSTCHAT_ENV_FILE (default deployment/.env), RUSTCHAT_PROJECT (default rustchat).
set -eu

out="${1:-./backups}"
env_file="${RUSTCHAT_ENV_FILE:-deployment/.env}"
project="${RUSTCHAT_PROJECT:-rustchat}"
name="rustchat-$(date +%Y%m%d-%H%M%S).dump"

mkdir -p "$out"
partial="$out/$name.partial"
trap 'rm -f "$partial"' EXIT

if [ -n "${PGHOST:-}" ]; then
    # shellcheck disable=SC2086 # the prefix is intentionally word-split
    ${RUSTCHAT_PG_PREFIX:-} pg_dump -Fc -d "${PGDATABASE:-rustchat}" > "$partial"
else
    docker compose --project-name "$project" --env-file "$env_file" -f deployment/compose.yml \
        exec -T db pg_dump -U rustchat -d rustchat -Fc > "$partial"
fi

# An empty or truncated file must never look like a good backup.
if [ ! -s "$partial" ]; then
    echo "backup failed: the dump is empty" >&2
    exit 1
fi
mv "$partial" "$out/$name"
trap - EXIT
echo "Backup written to $out/$name"
