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

Small, testable first slice. Because accounts have no email, registration necessarily reveals when a username is taken, and there is no password recovery yet: a user who forgets a password cannot regain the account. Both are accepted for now and listed in the threat model. Open registration plus rate limiting ([ADR 0006](0006-in-process-rate-limiting-behind-a-port.md)) is the interim abuse control. Revisit when email is added; that change will also revisit enumeration behaviour.
