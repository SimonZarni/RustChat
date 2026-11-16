# 0008. Explicit database migrations

- **Status:** Accepted
- **Date:** 2026-11-14

## Context

Schema changes must be versioned and safe to roll out, with old and new application versions possibly overlapping ([MIGRATIONS.md](../MIGRATIONS.md)). Running DDL automatically at startup couples deploys to schema changes and requires the application's database role to have DDL rights.

## Decision

Use SQLx migrations embedded in the server binary and applied by an explicit, idempotent command: `rustchat-server migrate`. The server never migrates on startup. Migration files are forward-only; rollback means restoring a backup or shipping a corrective migration.

## Alternatives considered

- **Migrate on startup:** convenient for development, but removes the operator's control over timing and over which role holds DDL rights.
- **A separate migration tool and files outside the binary:** more moving parts for self-hosters.
- **Reversible (down) migrations:** rarely tested in practice and unsafe for data-destroying changes.

## Consequences

Deploy procedures must run `migrate` before starting a new version; the README and Compose instructions say so. Every integration test creates a fresh database through the same migrations, which exercises them continuously. The production Compose stack uses separate migration and application database roles; the local development stack keeps setup simpler. Revisit if self-hosters find the explicit migration step burdensome, for example by adding an opt-in startup flag.
