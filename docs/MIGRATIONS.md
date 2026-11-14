# RustChat Migration Strategy

## 1. Status

No migrations exist yet. This document fixes the conventions so the first migration follows them. It complements [DATABASE.md](DATABASE.md) section 10.

## 2. Tooling

- SQLx migrations, stored in `backend/migrations/` as forward-only SQL files named `NNNN_description.sql`. Rollback is by restoring a backup or shipping a corrective forward migration; the exact policy is recorded per risky migration.
- Migrations run explicitly (a dedicated command or startup step chosen at implementation time), never implicitly on every request path.
- The application connects with a least-privilege role; a separate role with DDL rights runs migrations.

## 3. Rules

1. Never edit a migration that has been applied anywhere outside a developer machine. Add a new one.
2. Use expand/contract for breaking schema changes so old and new application versions can overlap: add nullable/new structures, deploy, backfill, then remove old structures in a later release.
3. Keep migrations short. Run large backfills as separate, resumable, batched jobs.
4. Create indexes on large tables with `CREATE INDEX CONCURRENTLY` in a migration that does not run in a transaction.
5. Add constraints that express real invariants (foreign keys, `CHECK`, unique keys such as sender + client message ID).
6. Document operationally risky migrations and their rollback limits in the PR.

## 4. Testing

- CI applies all migrations to an empty PostgreSQL database.
- CI (once at least one released schema exists) also upgrades a database created from the previous release.
- Integration tests run against real PostgreSQL, not mocks.

## 5. Open items

- Choose the migration execution model (separate command vs. startup step).
- Decide the PostgreSQL version range to support (the dev compose file uses `postgres:18`).
