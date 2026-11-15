# 0010. Optional email-based password recovery

- **Status:** Accepted
- **Date:** 2026-11-15
- **Supersedes:** [ADR 0005](0005-username-password-accounts-with-configurable-registration.md) for email and password recovery policy

## Context

The MVP requires a password recovery path. Email verification is explicitly deferred, but accounts need a recovery address when the operator configures SMTP. The request path must not reveal whether an address belongs to an account.

## Decision

- Registration may store an optional normalized email address; the web registration form requests one. Email verification and email changes are deferred.
- SMTP delivery is optional at deployment. Password recovery endpoints return `503 EMAIL_UNAVAILABLE` when SMTP is not configured.
- Reset requests use generic `202` responses for unknown and eligible addresses. Requests are rate limited by client network and a digest of the normalized email address.
- Reset tokens are random, expire after one hour, and are stored only as SHA-256 digests. Issuing a new token invalidates prior outstanding reset tokens.
- Token consumption, password update, account reactivation, and existing-session revocation run in one database transaction. Tokens are single-use.
- Delivery failures are logged without returning account-specific information to the requester.

## Consequences

Deployments that need account recovery must configure SMTP and a public base URL. Accounts without a recovery address cannot use self-service reset. Because addresses are not verified, operators should treat the stored address as a recovery destination chosen at registration; email verification and address changes require a later policy decision.

## Alternatives considered

- **Require verification before reset:** secure ownership check, but conflicts with the decision to defer email verification and blocks recovery until that feature exists.
- **Allow reset without email:** requires a separate operator-managed recovery process and adds more account support work to the MVP.
