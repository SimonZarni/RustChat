# 0012. Email verification gates account recovery

- **Status:** Accepted
- **Date:** 2026-11-16
- **Supersedes:** [ADR 0010](0010-optional-email-password-recovery.md) for verification and address changes

## Context

ADR 0010 stored an optional recovery address without verifying it and listed "require verification before reset" as an alternative that conflicted with deferring verification. The product owner has since decided to build verification. Without it, anyone can attach someone else's address to an account (spamming them with reset mail) and, with a global uniqueness rule, block the real owner from ever using it.

## Decision

- An address is verified when its owner follows a 24-hour, single-use link sent to it. Tokens are 256-bit random values; only their SHA-256 digest is stored, in `account_tokens` with a `purpose` (`password_reset` or `email_verification`). A verification token is bound to the address it verifies, so changing the address invalidates it.
- Password reset is only sent to a verified address. Addresses stored before this change are unverified until their owners confirm them.
- Uniqueness applies to verified addresses only. Registering with an address someone else has not verified succeeds without any "address taken" signal. If two accounts try to verify the same address, the first wins and the second link fails with `EMAIL_ADDRESS_IN_USE`.
- Owners can set, change or remove the address after re-entering their password. A change clears verification and discards outstanding tokens of both purposes.
- A verification email is sent at registration (when SMTP is configured) and can be resent at most three times per hour per account. Verifying a link needs no session and is rate limited per client network.
- No behaviour depends on verification other than password recovery for now. Verification does not gate sign-in.

## Alternatives considered

- **Keep verification deferred:** simplest, but leaves address squatting and unwanted reset mail.
- **Block sign-in until verified:** stronger, but locks out operators who run without SMTP, which remains a supported configuration.
- **Keep global uniqueness:** simpler constraint, but lets an attacker deny an address to its owner and makes registration an address-existence oracle.

## Consequences

Accounts that registered with an address before this decision cannot use self-service recovery until they confirm it in their settings; operators should tell their users. Verification needs working SMTP and a correct `RUSTCHAT_PUBLIC_URL`. Several accounts can hold the same unverified address, which is intentional.
