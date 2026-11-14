# RustChat Migration Strategy

## 1. Status

One migration exists: `0001_identity.sql` (users and sessions). This document fixes the conventions that migrations follow. It complements [DATABASE.md](DATABASE.md) section 10.

## 2. Tooling

- SQLx migrations, stored in `backend/migrations/` as forward-only SQL files named `NNNN_description.sql`. Rollback is by restoring a backup or shipping a corrective forward migration; the exact policy is recorded per risky migration.
- Migrations are embedded in the server binary and run explicitly with `rustchat-server migrate` (idempotent). The server does not migrate on startup, so a deploy decides when the schema changes.
- Target model: the application connects with a least-privilege role and a separate role with DDL rights runs `migrate`. Today the local development setup uses one role for both.

## 3. Rules

1. Never edit a migration that has been applied anywhere outside a developer machine. Add a new one.
2. Use expand/contract for breaking schema changes so old and new application versions can overlap: add nullable/new structures, deploy, backfill, then remove old structures in a later release.
3. Keep migrations short. Run large backfills as separate, resumable, batched jobs.
4. Create indexes on large tables with `CREATE INDEX CONCURRENTLY` in a migration that does not run in a transaction.
5. Add constraints that express real invariants (foreign keys, `CHECK`, unique keys such as sender + client message ID).
6. Document operationally risky migrations and their rollback limits in the PR.

## 4. Testing

- CI applies all migrations to an empty PostgreSQL database: every `sqlx::test` creates a fresh database and runs the full migration set.
- CI (once at least one released schema exists) also upgrades a database created from the previous release.
- Integration tests run against real PostgreSQL, not mocks.

## 5. Open items

- Create the separate migration and application database roles.
- Decide the PostgreSQL version range to support (the dev compose file uses `postgres:18`).
