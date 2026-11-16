# Self-hosted deployment baseline

The Compose stack builds the Rust API and web client, serves HTTPS through Caddy, and keeps PostgreSQL off the host network. It is a single-instance baseline: authentication rate limits are shared through PostgreSQL, while WebSocket fan-out and presence remain process-local.

## First start

1. Install Docker Engine with the Compose plugin and point `RUSTCHAT_DOMAIN` at this host.
2. Copy `deployment/.env.example` to `deployment/.env` and replace all passwords and the domain with unique values. Passwords must use letters, digits, `_` or `-` because Compose embeds them in database URLs. Keep `.env` private and out of source control.
3. Start the database and apply migrations: `docker compose --env-file deployment/.env -f deployment/compose.yml up -d db`, then `docker compose --env-file deployment/.env -f deployment/compose.yml --profile operations run --rm migrate`.
4. Start the application: `docker compose --env-file deployment/.env -f deployment/compose.yml up -d --build`.
5. Verify `https://$RUSTCHAT_DOMAIN/healthz` and `/readyz` return `{"status":"ok"}` and register an account. Compose waits for PostgreSQL and server readiness before starting Caddy.

## Configure the first instance administrator

Instance administrators are explicitly assigned by account UUID; community roles do not grant instance-wide authority. After registering the intended administrator account, sign in and call `GET /api/v1/auth/me` to obtain its `user.id`. Put that UUID in `RUSTCHAT_ADMIN_USER_IDS` in `deployment/.env` (comma-separate multiple IDs), then recreate the server with `docker compose --env-file deployment/.env -f deployment/compose.yml up -d --force-recreate server`. The administration panel and `/api/v1/admin/users` become available to those accounts after restart. Keep the ID list limited to trusted operators.

Caddy obtains and renews certificates automatically when public DNS and inbound ports 80/443 reach the host. For local-only evaluation, use `localhost` and configure Caddy's local certificate trust as described by Caddy before relying on secure cookies.

## Backups and restore

The runtime API and schema migrator use separate database credentials from the PostgreSQL bootstrap superuser. The role initialization script runs only when PostgreSQL creates a new data volume; existing databases need an explicit role/grant migration before using this Compose stack.

Create a PostgreSQL custom-format backup with `./deployment/backup.ps1`. Store copies outside the host and protect them as sensitive data; they contain account, session, community and message records. The script writes into `./backups` by default. Backups are not validated merely by existing. During the Phase 1 audit, the documented workflow was exercised with a fresh Compose volume: all migrations ran, two accounts exchanged a message, a backup was restored, and the restored health endpoint, session, and message history were verified. Repeat this process on the target host and on a scheduled basis before relying on it for operations.

To restore, stop the server and run `./deployment/restore.ps1 -BackupFile <path>`. This replaces database contents. Afterward, start the stack and check health and sign-in. For operational validation, restore a recent backup into an isolated test instance and verify migrations, account sign-in, and message history before relying on the backup process.

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

## Operations and limits

- Migrations are explicit: run the `migrate` profile before starting a release that adds migrations. The server does not migrate automatically.
- Keep release-specific image tags and a pre-upgrade database backup; this runbook does not make schema migrations reversible.
- PostgreSQL data and Caddy certificates/configuration persist in named volumes.
- Use a firewall, restrict host access, rotate secrets, monitor disk space, and retain encrypted off-host backups.
- The server exposes low-cardinality HTTP response totals at `/metrics` on the private Compose network; Caddy blocks external access. This baseline does not provide multi-instance fan-out, external rate limiting, alerting, or a tested disaster-recovery objective. Those need a separate operational phase.
