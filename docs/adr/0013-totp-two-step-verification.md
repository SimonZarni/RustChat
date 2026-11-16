# 0013. Optional TOTP two-step verification

- **Status:** Accepted
- **Date:** 2026-11-16
- **Supersedes:** the MFA deferral in [ADR 0005](0005-username-password-accounts-with-configurable-registration.md)

## Context

ADR 0005 deferred multi-factor authentication to a later security milestone, and the PRD left its scope open. A password is the only credential today; a leaked or reused password gives full access. The product owner asked for MFA now.

## Decision

- Support **time-based one-time passwords** (RFC 6238, six digits, 30-second steps, one step of clock skew) with any standard authenticator app. WebAuthn/passkeys remain a candidate for later and would be added beside it.
- Use maintained libraries and write no cryptography: `totp-rs` for the algorithm and `aes-gcm` (RustCrypto) to protect stored secrets.
- The authenticator secret must be recoverable to verify codes, so it is stored **encrypted with AES-256-GCM** under a 256-bit server key from `RUSTCHAT_MFA_ENCRYPTION_KEY`, with a random nonce and the owning user id as associated data. The key is never stored in the database. With no key configured the feature is off and login is unchanged.
- Enrolment is two-step: the user re-enters their password to receive a secret, then proves the authenticator works with a valid code; only then does it affect login. Confirmation issues ten **recovery codes** (about 80 bits each, shown once, stored only as SHA-256 digests, single use).
- Login becomes two-step for enrolled accounts: the password step returns a **challenge** (a random bearer token stored as a digest, five-minute lifetime, five attempts) and no session; the second step takes a TOTP or recovery code and then issues the session. Failures are indistinguishable (`INVALID_MFA_CODE`) and spend the same per-account failure budget as wrong passwords.
- Each TOTP time step is accepted once (compare-and-set on the last accepted step) and each recovery code once.
- Disabling it or replacing recovery codes needs the password **and** a current code, so a stolen session cannot remove the second factor. A password reset does not bypass it.
- No enforcement policy yet: MFA is opt-in per account.

## Alternatives considered

- **SMS or email codes:** weaker (SIM swap, mailbox compromise) and need delivery infrastructure.
- **WebAuthn first:** stronger against phishing but a larger client and protocol surface; better as a follow-up.
- **Store the secret in plaintext, or hash it:** plaintext exposes every second factor in a database leak; a hash cannot verify codes.
- **Derive the key from a user secret:** would stop the server verifying codes without the password, which login needs.

## Consequences

Operators gain a new secret to protect and back up separately from the database; losing it forces affected users onto their recovery codes and re-enrolment, and key rotation is not implemented. Users who lose both their device and their recovery codes need out-of-band help from an administrator. Authenticator apps are not phishing-resistant: a user tricked into typing a code on a fake page can be impersonated for that step.
