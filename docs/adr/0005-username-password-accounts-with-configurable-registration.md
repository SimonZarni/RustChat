# 0005. Username/password accounts and configurable registration

- **Status:** Superseded by 0010
- **Date:** 2026-11-14

## Context

The PRD left open the initial registration policy and whether email verification is mandatory. Email needs a mail-sending dependency, reset-token handling and an SMTP server for development, which would enlarge the first slice.

## Decision

- Accounts are identified by a case-insensitive username (3 to 32 characters, `[a-z0-9_]`) and a password (12 to 128 characters, hashed with Argon2id).
- Registration defaults to open and can be switched to closed with `RUSTCHAT_REGISTRATION=closed`. Invite-only and admin-created accounts are added when the invitation and admin features exist.
- Email verification is deferred. Optional email is supported for password recovery; recovery requires configured SMTP. MFA is deferred to a later security milestone.

## Alternatives considered

- **Invite-only first:** depends on the invitation system, which is not built.
- **Email as the login identifier:** hides whether a username is taken, but pulls in mail infrastructure immediately.

## Consequences

At the time this ADR was recorded, accounts had no recovery email and no password-reset path. Those consequences were superseded by [ADR 0010](0010-optional-email-password-recovery.md), which added optional recovery email and reset. Registration still reveals taken usernames. Open registration plus rate limiting remains the default, and the initial in-process limiter described in [ADR 0006](0006-in-process-rate-limiting-behind-a-port.md) was later replaced for production by [ADR 0011](0011-postgresql-shared-auth-rate-limits.md).
