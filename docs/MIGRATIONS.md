# RustChat Migration Strategy

## 1. Status

Twenty-four migrations currently exist: `0001_identity.sql` (users and sessions), `0002_conversations.sql` (conversations and members), `0003_messages.sql` (messages, sequences and outbox events), `0004_communities.sql` (communities, channels, roles, overrides and invites), `0005_message_edits.sql` (message edit/delete timestamps), `0006_outbox_event_revisions.sql` (allowing repeated message updates at one sequence), `0007_community_audit.sql` (administrative audit events), `0008_user_profiles.sql` (display names and bios), `0009_read_cursors.sql` (member-scoped durable read positions), `0010_message_reactions.sql` (member-scoped emoji reactions), `0011_delivery_cursors.sql` (member-scoped delivery acknowledgements), `0012_message_replies.sql` (conversation-scoped reply references), `0013_message_mentions.sql` (member-scoped message mentions), `0014_channel_categories.sql` (community channel categories and channel assignment), `0015_profile_privacy.sql` (profile discoverability and visibility controls), `0016_account_lifecycle.sql` (deactivation state and retained deletion tombstones), `0017_presence_privacy.sql` (user-configurable online-status visibility), `0018_account_recovery.sql` (optional recovery email and single-use account recovery token storage; email verification is added by `0022`), `0019_shared_rate_limits.sql` (hashed, expiring authentication rate-limit keys shared through PostgreSQL), `0020_instance_administration.sql` (account suspension state and instance-admin audit events), `0021_session_lifecycle.sql` (session activity, token rotation and its grace period), `0022_email_verification.sql` (verified recovery addresses and token purposes), `0023_mfa.sql` (encrypted TOTP secrets, recovery codes and login challenges), and `0024_multi_instance_realtime.sql` (outbox leases and shared presence). This document fixes the conventions that migrations follow. It complements [DATABASE.md](DATABASE.md) section 10.

## 2. Tooling

- SQLx migrations, stored in `backend/migrations/` as forward-only SQL files named `NNNN_description.sql`. Rollback is by restoring a backup or shipping a corrective forward migration; the exact policy is recorded per risky migration.
- Migrations are embedded in the server binary and run explicitly with `rustchat-server migrate` (idempotent). The server does not migrate on startup, so a deploy decides when the schema changes.
- The deployment Compose setup uses a least-privilege runtime role and a separate migration role with DDL rights. Local development uses one role for both to keep setup simple.

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

- Decide the PostgreSQL version range to support (the dev compose file uses `postgres:18`).
