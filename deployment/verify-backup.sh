#!/bin/sh
# Prove that a backup can actually be restored, without touching the live database.
#
#   deployment/verify-backup.sh <backup-file>
#
# The dump is restored into a throwaway database on the same PostgreSQL server,
# a set of consistency checks runs against it, and the scratch database is dropped
# again. It exits non-zero (and says why) when anything is wrong, so it suits cron
# and CI. It also prints how long the restore took, which is a measured lower bound
# for your recovery time.
#
# Checks: the file is readable and recent enough, pg_restore succeeds, every
# migration is recorded and successful, the core tables exist, no message points
# at a missing conversation, conversation counters are not behind their messages,
# the application role can use the restored tables, and the backup contains users.
#
# Connection: by default the Compose stack's database through `docker compose exec`.
# Set PGHOST (plus PGPORT, PGUSER, PGPASSWORD) to use a PostgreSQL server directly;
# the user needs CREATEDB. RUSTCHAT_PG_PREFIX can name a command that runs the
# client tools, for example a container with a matching PostgreSQL version.
#
# Environment:
#   RUSTCHAT_ENV_FILE, RUSTCHAT_PROJECT   Compose file and project (deployment/.env, rustchat)
#   RUSTCHAT_EXPECTED_MIGRATIONS          expected migration count; default: the .sql files in
#                                         backend/migrations when that directory exists
#   RUSTCHAT_MAX_BACKUP_AGE_HOURS         fail when the file is older than this
#   RUSTCHAT_ALLOW_EMPTY_BACKUP=1         accept a backup with no users (a brand new instance)
set -eu

if [ $# -ne 1 ] || [ ! -f "$1" ]; then
    echo "usage: $0 <backup-file>" >&2
    exit 2
fi
dump="$1"
env_file="${RUSTCHAT_ENV_FILE:-deployment/.env}"
project="${RUSTCHAT_PROJECT:-rustchat}"
admin="${PGUSER:-rustchat}"
scratch="rustchat_verify_$$_$(date +%s)"

if [ -n "${PGHOST:-}" ]; then
    pg() {
        # shellcheck disable=SC2086 # the prefix is intentionally word-split
        ${RUSTCHAT_PG_PREFIX:-} "$@"
    }
else
    pg() {
        docker compose --project-name "$project" --env-file "$env_file" -f deployment/compose.yml exec -T db "$@"
    }
fi
sql() { pg psql -U "$admin" -d "$1" -v ON_ERROR_STOP=1 -tAq -c "$2"; }

failures=0
fail() {
    echo "FAIL: $1" >&2
    failures=$((failures + 1))
}
check_zero() { # description, database, query that must return 0
    value=$(sql "$2" "$3" 2>&1) || { fail "$1 (query failed: $value)"; return; }
    if [ "$value" = "0" ]; then echo "ok:   $1"; else fail "$1 (found $value)"; fi
}

cleanup() {
    sql postgres "DROP DATABASE IF EXISTS \"$scratch\" WITH (FORCE)" >/dev/null 2>&1 || \
        echo "warning: could not drop scratch database $scratch; drop it manually" >&2
}

# --- the file itself ---
if [ ! -s "$dump" ]; then
    echo "FAIL: $dump is empty" >&2
    exit 1
fi
if [ -n "${RUSTCHAT_MAX_BACKUP_AGE_HOURS:-}" ]; then
    # find -mmin is POSIX enough on Linux and macOS userlands
    limit=$((RUSTCHAT_MAX_BACKUP_AGE_HOURS * 60))
    if [ -n "$(find "$dump" -mmin "+$limit" 2>/dev/null)" ]; then
        fail "the backup is older than $RUSTCHAT_MAX_BACKUP_AGE_HOURS hours"
    else
        echo "ok:   the backup is newer than $RUSTCHAT_MAX_BACKUP_AGE_HOURS hours"
    fi
fi

# --- restore into a scratch database ---
trap cleanup EXIT
sql postgres "CREATE DATABASE \"$scratch\"" >/dev/null
started=$(date +%s)
if pg pg_restore -U "$admin" -d "$scratch" --no-owner < "$dump" >/dev/null 2>"${TMPDIR:-/tmp}/rustchat-restore-$$.log"; then
    elapsed=$(( $(date +%s) - started ))
    echo "ok:   pg_restore completed in ${elapsed}s"
else
    fail "pg_restore reported errors: $(head -c 600 "${TMPDIR:-/tmp}/rustchat-restore-$$.log" | tr '\n' ' ')"
fi
rm -f "${TMPDIR:-/tmp}/rustchat-restore-$$.log"

# --- consistency checks on the restored copy ---
expected="${RUSTCHAT_EXPECTED_MIGRATIONS:-}"
if [ -z "$expected" ] && [ -d backend/migrations ]; then
    expected=$(find backend/migrations -maxdepth 1 -name '*.sql' | wc -l | tr -d ' ')
fi
recorded=$(sql "$scratch" "SELECT count(*) FROM _sqlx_migrations WHERE success" 2>&1) || recorded="unreadable"
if [ -n "$expected" ]; then
    if [ "$recorded" = "$expected" ]; then echo "ok:   $recorded of $expected migrations recorded"; else fail "migrations: expected $expected recorded successfully, found $recorded"; fi
else
    echo "note: set RUSTCHAT_EXPECTED_MIGRATIONS to check the migration count (found $recorded)"
fi
check_zero "no migration is marked failed" "$scratch" "SELECT count(*) FROM _sqlx_migrations WHERE NOT success"

for table in users sessions conversations conversation_members messages outbox_events communities; do
    check_zero "table $table exists" "$scratch" "SELECT CASE WHEN to_regclass('public.$table') IS NULL THEN 1 ELSE 0 END"
done
check_zero "no message refers to a missing conversation" "$scratch" \
    "SELECT count(*) FROM messages m LEFT JOIN conversations c ON c.id = m.conversation_id WHERE c.id IS NULL"
check_zero "no conversation counter is behind its messages" "$scratch" \
    "SELECT count(*) FROM conversations c WHERE c.last_sequence < COALESCE((SELECT max(sequence) FROM messages m WHERE m.conversation_id = c.id), 0)"
check_zero "no session belongs to a missing user" "$scratch" \
    "SELECT count(*) FROM sessions s LEFT JOIN users u ON u.id = s.user_id WHERE u.id IS NULL"

role_exists=$(sql postgres "SELECT count(*) FROM pg_roles WHERE rolname = 'rustchat_app'" 2>/dev/null || echo 0)
if [ "$role_exists" = "1" ]; then
    check_zero "the application role can use the restored tables" "$scratch" \
        "SELECT CASE WHEN has_table_privilege('rustchat_app', 'public.users', 'SELECT,INSERT,UPDATE') AND has_table_privilege('rustchat_app', 'public.messages', 'SELECT,INSERT') THEN 0 ELSE 1 END"
else
    echo "note: no rustchat_app role on this server; skipped the application-role check"
fi

users=$(sql "$scratch" "SELECT count(*) FROM users" 2>/dev/null || echo 0)
messages=$(sql "$scratch" "SELECT count(*) FROM messages" 2>/dev/null || echo 0)
echo "info: the backup holds $users users and $messages messages"
if [ "$users" = "0" ] && [ "${RUSTCHAT_ALLOW_EMPTY_BACKUP:-0}" != "1" ]; then
    fail "the backup contains no users (set RUSTCHAT_ALLOW_EMPTY_BACKUP=1 for a new instance)"
fi

if [ "$failures" -gt 0 ]; then
    echo "BACKUP NOT VERIFIED: $failures problem(s)" >&2
    exit 1
fi
echo "BACKUP VERIFIED: $dump restores cleanly (restore took ${elapsed:-?}s on this server)"
