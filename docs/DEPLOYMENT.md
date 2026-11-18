# Self-hosted deployment baseline

The Compose stack builds the Rust API and web client, serves HTTPS through Caddy, and keeps PostgreSQL off the host network. One `server` container is the default, and several can run against the same database (see "Running more than one server instance"): authentication rate limits, realtime events, outbox dispatch and presence are all coordinated through PostgreSQL.

## First start

1. Install Docker Engine with the Compose plugin and point `RUSTCHAT_DOMAIN` at this host.
2. Copy `deployment/.env.example` to `deployment/.env` and replace all passwords and the domain with unique values. Passwords must use letters, digits, `_` or `-` because Compose embeds them in database URLs. Keep `.env` private and out of source control.
3. Start the database and apply migrations: `docker compose --env-file deployment/.env -f deployment/compose.yml up -d db`, then `docker compose --env-file deployment/.env -f deployment/compose.yml --profile operations run --rm migrate`.
4. Start the application: `docker compose --env-file deployment/.env -f deployment/compose.yml up -d --build`.
5. Verify `https://$RUSTCHAT_DOMAIN/healthz` and `/readyz` return `{"status":"ok"}` and register an account. Compose waits for PostgreSQL and server readiness before starting Caddy.

## Configuration reference

`deployment/.env` is read by Compose. Settings you can change there:

| Variable | Default | Purpose |
|---|---|---|
| `RUSTCHAT_DOMAIN` | required | Public host name; Caddy serves HTTPS for it and `RUSTCHAT_ALLOWED_ORIGINS` is set to `https://<domain>` |
| `RUSTCHAT_REGISTRATION` | `open` | `open` or `closed` account registration |
| `RUSTCHAT_DB_MAX_CONNECTIONS` | `10` | PostgreSQL pool size |
| `RUSTCHAT_GROUP_MEMBER_LIMIT` | `100` | Group conversation cap, 2–500 |
| `RUSTCHAT_MFA_ENCRYPTION_KEY`, `RUSTCHAT_MFA_ISSUER` | empty, `RustChat` | Two-step verification with authenticator apps; see below |
| `RUSTCHAT_SESSION_IDLE_TIMEOUT_DAYS` | `7` | Days of inactivity after which a session ends, 1–30 (the absolute lifetime is 30 days) |
| `RUSTCHAT_LOGIN_PER_IP_LIMIT`, `RUSTCHAT_LOGIN_PER_USERNAME_LIMIT`, `RUSTCHAT_REGISTER_PER_IP_LIMIT` | `30`, `10`, `5` | Authentication rate-limit thresholds (windows are fixed) |
| `RUSTCHAT_API_READ_LIMIT`, `RUSTCHAT_API_WRITE_LIMIT`, `RUSTCHAT_MESSAGE_SEND_LIMIT`, `RUSTCHAT_CREATE_LIMIT`, `RUSTCHAT_UNAUTHENTICATED_LIMIT` | `600`, `120`, `60`, `30`, `120` | General API budgets: reads, other writes and message sends per user per minute, creations per user per hour, and failed-authentication requests per client network per minute. Counted in each server process's memory, so they apply per instance |
| `RUSTCHAT_ADMIN_USER_IDS` | empty | Instance administrator account UUIDs (see below) |
| `RUSTCHAT_ATTACHMENT_MAX_MIB`, `RUSTCHAT_ATTACHMENT_QUOTA_GIB` | `25`, `2` | Maximum size per file (1–100 MiB) and ready-file quota per user (1–100000 GiB) |
| `RUSTCHAT_S3_ENDPOINT`, `RUSTCHAT_S3_REGION`, `RUSTCHAT_S3_BUCKET`, `RUSTCHAT_S3_ACCESS_KEY_ID`, `RUSTCHAT_S3_SECRET_ACCESS_KEY` | empty | Configure all five to enable attachments; Garage is selected but must be provisioned separately |
| `RUSTCHAT_SERVER_IMAGE`, `RUSTCHAT_WEB_IMAGE` | local build tags | Release image tags |

Set by `compose.yml` and not meant to be changed: `RUSTCHAT_BIND`, `RUSTCHAT_COOKIE_SECURE=true`, `RUSTCHAT_ALLOWED_ORIGINS`, and `RUSTCHAT_TRUSTED_PROXY_HOPS=1`. The last one makes the server take the client address from the `X-Forwarded-For` entry that Caddy appends, so authentication rate limits apply per client rather than to Caddy's address as a whole. It is safe only because port 8080 is not published; do not expose the server port directly or put another proxy in front of Caddy without adjusting it.

**Attachments:** provisioning a private S3-compatible bucket and least-privilege credentials is an operator step; Compose does not run Garage. Set all five S3 variables together or leave them all empty (attachments then return `503`). The endpoint is contacted directly by browsers for signed uploads/downloads, so it must be publicly reachable over HTTPS and configured with bucket CORS for the exact RustChat origin, `PUT` and `GET`, and the signed `Content-Type` header. Apply an object-store lifecycle rule to expire the `staging/` prefix after a short period. PostgreSQL backup scripts do not include object bytes: back up and restore the bucket separately, coordinating it with database snapshots so metadata and files stay consistent. The signed upload/download flow was exercised against a local Garage instance; production deployment and object-store recovery have not been rehearsed.

To exercise the attachment routes against PostgreSQL and a configured S3-compatible service, set `DATABASE_URL` and the five `RUSTCHAT_S3_*` variables in the test shell, then run `cargo test -p rustchat-http --test attachments_api -- --ignored` from `backend/`. This covers signed upload, completion, message association, authorized member download and outsider rejection. The test's bucket is not created automatically.

**Two-step verification** is off until you set `RUSTCHAT_MFA_ENCRYPTION_KEY` to 64 hexadecimal characters (`openssl rand -hex 32`). The key encrypts each user's authenticator secret before it is stored (AES-256-GCM, bound to the account), so a copy of the database alone does not reveal them. Treat it like a password: keep it out of source control and **back it up separately from the database** — if it is lost or changed, enrolled authenticators can no longer be verified and users can only sign in with their recovery codes, after which they must set up again. Rotating the key is not supported yet. Without a key the settings screen says the server has not been set up for authenticator apps, and sign-in is unchanged.

**Password-recovery email** is optional and off by default. Set `RUSTCHAT_SMTP_HOST`, `RUSTCHAT_EMAIL_FROM` and `RUSTCHAT_PUBLIC_URL` (the HTTPS URL used in reset links) together; the server refuses to start with only some of them. `RUSTCHAT_SMTP_PORT` defaults to `465`. `RUSTCHAT_SMTP_USERNAME` and `RUSTCHAT_SMTP_PASSWORD` must be set together or not at all. Without this configuration the password-reset request endpoint returns `503 EMAIL_UNAVAILABLE`.

## Configure the first instance administrator

Instance administrators are explicitly assigned by account UUID; community roles do not grant instance-wide authority. After registering the intended administrator account, sign in and call `GET /api/v1/auth/me` to obtain its `user.id`. Put that UUID in `RUSTCHAT_ADMIN_USER_IDS` in `deployment/.env` (comma-separate multiple IDs), then recreate the server with `docker compose --env-file deployment/.env -f deployment/compose.yml up -d --force-recreate server`. The administration panel and `/api/v1/admin/users` become available to those accounts after restart. Keep the ID list limited to trusted operators.

Caddy obtains and renews certificates automatically when public DNS and inbound ports 80/443 reach the host. For local-only evaluation, use `localhost` and configure Caddy's local certificate trust as described by Caddy before relying on secure cookies.

## Backups and restore

The runtime API and schema migrator use separate database credentials from the PostgreSQL bootstrap superuser. The role initialization script runs only when PostgreSQL creates a new data volume; existing databases need an explicit role/grant migration before using this Compose stack.

Create a PostgreSQL custom-format backup with `./deployment/backup.ps1`. Store copies outside the host and protect them as sensitive data; they contain account, session, community and message records. The script writes into `./backups` by default. Backups are not validated merely by existing. During the Phase 1 audit, the documented workflow was exercised with a fresh Compose volume: all migrations ran, two accounts exchanged a message, a backup was restored, and the restored health endpoint, session, and message history were verified. Repeat this process on the target host and on a scheduled basis before relying on it for operations.

To restore, stop the server and run `./deployment/restore.ps1 -BackupFile <path>`. This replaces database contents. Afterward, start the stack and check health and sign-in. For operational validation, restore a recent backup into an isolated test instance and verify migrations, account sign-in, and message history before relying on the backup process.

### Verifying backups automatically

A backup you have never restored is a hope, not a backup. `deployment/verify-backup.sh <file>` restores a dump into a **throwaway database** on the same PostgreSQL server (the live database is never touched), runs consistency checks, drops the scratch database and exits non-zero if anything is wrong. It checks that the file is readable (and, with `RUSTCHAT_MAX_BACKUP_AGE_HOURS`, recent), that `pg_restore` finishes without errors, that every migration is recorded as successful (matching the number of migration files when run from the repository, or `RUSTCHAT_EXPECTED_MIGRATIONS`), that the core tables exist, that no message points at a missing conversation, that conversation counters are not behind their messages, that the application database role can use the restored tables, and that the backup contains users (`RUSTCHAT_ALLOW_EMPTY_BACKUP=1` for a brand new instance). It prints how long the restore took.

```sh
RUSTCHAT_ENV_FILE=deployment/.env deployment/backup.sh /srv/rustchat-backups        # take a backup (Linux and macOS)
RUSTCHAT_ENV_FILE=deployment/.env RUSTCHAT_MAX_BACKUP_AGE_HOURS=26     deployment/verify-backup.sh "$(ls -t /srv/rustchat-backups/*.dump | head -1)"     # verify the newest one
```

Run both from cron (for example backup at 02:00, verify at 02:30) and alert on a non-zero exit. `backup.sh`, `restore.sh` and `verify-backup.sh` are the POSIX counterparts of the PowerShell scripts; set `PGHOST` (and `PGUSER`, `PGPASSWORD`) to use them against a PostgreSQL server directly instead of the Compose database. The continuous-integration `restore-check` job runs the same scripts on every change: it migrates a fresh database, creates data through the API, backs it up, verifies the restore, and confirms a damaged dump is rejected.

### Recovery objectives

These are targets for you to confirm on your own hardware, not guarantees:

- **Recovery point objective (RPO):** at most the time since the last good backup. With the daily schedule above, up to 24 hours of messages and changes can be lost. Run backups more often if that is too much; every dump is a full copy, so size and frequency trade against each other. There is no continuous archiving (point-in-time recovery) in this baseline.
- **Recovery time objective (RTO):** the time to start a database, restore the newest dump and restart the stack. `verify-backup.sh` reports the restore step; on a small test instance it took a few seconds, and it grows with the database size, so re-measure as yours grows. Add the time to provision a replacement host and restore DNS if the whole host is lost, and keep the `.env` file, the Caddy volume (certificates) and `RUSTCHAT_MFA_ENCRYPTION_KEY` backed up separately: a database dump alone does not restore two-step verification.
- Keep copies off the host, encrypted, and rehearse a full restore onto a spare machine at least once.

## Upgrade and rollback

Keep a copy of the current `deployment/.env` and the exact server and web image tags used by that release. Use a new, immutable tag for each release; do not reuse `latest` or overwrite a tag that may be needed for rollback. The example environment file has separate `RUSTCHAT_SERVER_IMAGE` and `RUSTCHAT_WEB_IMAGE` settings so Compose can retain both versions. It also exposes the database connection pool size, group member cap, and login/registration rate thresholds; rate-limit windows remain fixed in this baseline.

Upgrade in this order:

1. Build or publish the server image with its release tag and the web image with its matching release tag. Set both image values in `deployment/.env` and keep the previous environment file and images available.
2. Create a database backup with `./deployment/backup.ps1` and confirm that the backup file exists and is readable.
3. Stop the running API to avoid serving requests while applying schema changes: `docker compose --env-file deployment/.env -f deployment/compose.yml stop server`.
4. Apply the new schema with `docker compose --env-file deployment/.env -f deployment/compose.yml --profile operations run --rm migrate`.
5. Start the release: `docker compose --env-file deployment/.env -f deployment/compose.yml up -d server web`.
6. Check `https://$RUSTCHAT_DOMAIN/healthz`, sign-in, and a representative conversation workflow. Retain the previous images and backup until the release has been accepted.

Database migrations are forward-only in this baseline. If a release fails after a migration has changed the schema, stop the API, restore the pre-upgrade backup with `./deployment/restore.ps1 -BackupFile <backup-path>`, restore the saved environment file with the previous image tags, then start `server` and `web` again. This replaces the database with its pre-upgrade state, so writes made after that backup are lost. If the migration did not run or made no schema change, redeploy the previous tagged images without restoring the database. Test this rollback procedure on an isolated copy before using it for a production incident.

## Running more than one server instance

The server is stateless apart from PostgreSQL, so you can run several replicas for capacity or to upgrade without downtime: `docker compose --env-file deployment/.env -f deployment/compose.yml up -d --scale server=2`. Caddy discovers the replicas through Docker DNS and spreads connections across them; WebSocket clients need no sticky routing. Realtime events, outbox dispatch and presence are coordinated through PostgreSQL (ADR 0014), so a message sent through one replica reaches clients connected to another.

- Each replica opens its own database pool (`RUSTCHAT_DB_MAX_CONNECTIONS`, default 10) plus a connection that listens for events, so size PostgreSQL's `max_connections` for all replicas together.
- The general API rate limits (reads, writes, sends, creations) are counted per replica, so the effective limit grows with the replica count; the authentication limits are shared.
- A replica that crashes leaves its users "online" for up to a minute without an offline event, and requests in flight to a replica that is stopped can fail once (an HTTP 502) before Caddy stops using it; clients retry and WebSockets reconnect and resynchronize.
- Run the `migrate` job once, before starting new replicas of a release that adds migrations.

## Operations and limits

- Migrations are explicit: run the `migrate` profile before starting a release that adds migrations. The server does not migrate automatically.
- Keep release-specific image tags and a pre-upgrade database backup; this runbook does not make schema migrations reversible.
- PostgreSQL data and Caddy certificates/configuration persist in named volumes.
- Use a firewall, restrict host access, rotate secrets, monitor disk space, and retain encrypted off-host backups.
- The server exposes low-cardinality HTTP response totals at `/metrics` on the private Compose network; Caddy blocks external access. This baseline does not provide external rate limiting, alerting, or a tested disaster-recovery objective. Those need a separate operational phase.
