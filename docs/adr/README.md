# Architecture Decision Records

An ADR records one significant decision: what was decided, why, and what it costs. They answer "why is it like this?" months later, and they are how `CLAUDE.md` and `CONTRIBUTING.md` expect architectural changes to be proposed.

## When to write one

Write an ADR before or with a change that:

- alters the architecture rules in [ARCHITECTURE.md](../ARCHITECTURE.md) or adds infrastructure (a broker, a service, a new datastore);
- changes a security model, a protocol or a persistent data contract;
- chooses or replaces a core dependency;
- changes the licence or a policy that affects contributors.

Small implementation choices do not need one.

## Format

One Markdown file per decision, named `NNNN-short-title.md`, numbered in order and never renumbered. Use [0000-template.md](0000-template.md). An accepted ADR is not edited to change its meaning: write a new ADR that supersedes it and update the old one's status line.

Statuses: `Proposed`, `Accepted`, `Superseded by NNNN`, `Rejected`.

## Index

| ADR | Decision | Status |
|---|---|---|
| [0001](0001-record-architecture-decisions.md) | Record architecture decisions | Accepted |
| [0002](0002-modular-monolith-with-layered-crates.md) | Modular monolith with layered crates | Accepted |
| [0003](0003-garage-for-object-storage.md) | Garage for object storage | Accepted |
| [0004](0004-server-side-sessions-in-http-only-cookies.md) | Server-side sessions in HttpOnly cookies | Accepted |
| [0005](0005-username-password-accounts-with-configurable-registration.md) | Username and password accounts, configurable registration, email deferred | Accepted |
| [0006](0006-in-process-rate-limiting-behind-a-port.md) | In-process rate limiting behind a port | Accepted |
| [0007](0007-apache-2-licence.md) | Apache-2.0 licence | Accepted |
| [0008](0008-explicit-database-migrations.md) | Explicit database migrations | Accepted |

ADRs 0001 to 0008 were written on 2026-11-14, after the decisions had been made in earlier work; their dates reflect when they were recorded, not when the choice was first discussed.
