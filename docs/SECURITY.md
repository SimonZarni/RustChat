# RustChat Security Requirements

## 1. Purpose and status

Security is a core design requirement for RustChat. This document records baseline requirements and intended controls. It is not a security certification or a claim that unimplemented controls already exist.

Security-sensitive features must be reviewed and tested before release.

## 2. Security principles

- Enforce authorization on the backend for every protected operation.
- Use established cryptographic protocols and maintained implementations.
- Minimize collection and retention of sensitive data.
- Use least privilege for services, credentials and administrators.
- Apply safe resource limits to resist abuse and denial of service.
- Keep secrets out of source control, logs and client-visible errors.
- Document threat assumptions and limitations honestly.

## 3. Authentication and passwords

- Hash passwords with Argon2id using parameters appropriate to the deployment and current guidance.
- Never store plaintext or reversibly encrypted passwords.
- Use secure session identifiers generated with a cryptographically secure random source.
- Support session revocation and expiration.
- Consider WebAuthn and TOTP as MFA options.
- Apply rate limits and abuse controls to authentication endpoints.
- Avoid account-enumeration leaks in public authentication responses.

## 4. Web session security

For browser clients:

- Use Secure, HttpOnly and appropriately configured SameSite cookies.
- Apply CSRF protection to state-changing cookie-authenticated requests.
- Configure strict CORS policies.
- Use a Content Security Policy and safe output rendering to reduce XSS risk.
- Rotate session identifiers after authentication and privilege changes.
- Revoke sessions after logout, credential changes and administrative revocation as appropriate.

For desktop clients, store credentials using supported operating-system secure storage facilities rather than plaintext application files.

## 5. Authorization

- Check authorization on the backend for every resource and action.
- Scope database lookups to the authenticated principal and authorized resource.
- Prevent IDOR/BOLA vulnerabilities; knowing a UUID must not grant access.
- Recheck permissions for WebSocket subscriptions and sensitive events.
- Handle membership removal and role changes so stale connections do not retain access.
- Test both allowed and denied cases.

## 6. WebSocket security and resource controls

- Require authentication.
- Validate browser origins according to deployment configuration.
- Enforce message-size, connection, subscription and rate limits.
- Use bounded outbound queues and backpressure.
- Handle slow consumers without unbounded buffering.
- Revalidate or close connections when sessions are revoked.
- Do not accept client-supplied identity or permission claims as authoritative.

## 7. Database and application security

- Use parameterized SQL queries.
- Use least-privilege database accounts.
- Validate input at trust boundaries.
- Avoid leaking internal errors to clients.
- Protect against SQL injection, SSRF, request smuggling and resource exhaustion.
- Keep dependencies updated and review security advisories.
- Do not commit secrets or production data.

## 8. File and attachment security

- Enforce upload size and rate limits.
- Do not trust filename extensions or client-supplied MIME types.
- Inspect file content and use safe storage names.
- Store files outside executable application paths.
- Use short-lived, scoped upload/download authorization.
- Restrict access to private attachments.
- Consider malware scanning and safe preview generation.
- Stream data instead of buffering whole files in application memory.

## 9. Encryption in transit and at rest

- Require TLS for production HTTP and WebSocket traffic.
- Protect backups, disks and object storage using appropriate encryption-at-rest controls.
- Manage encryption keys separately from the data they protect.
- Define key rotation and recovery procedures.

TLS protects data in transit between endpoints; it does not by itself provide end-to-end encryption.

## 10. End-to-end encryption (planned milestone)

E2EE must be designed and implemented as a dedicated security project.

- Do not invent cryptographic algorithms or protocols.
- Evaluate established protocol families and maintained implementations. Signal-style protocols may suit 1:1 messaging; MLS (RFC 9420) is a candidate for group messaging.
- Audit maintenance, security review, platform support and licence before selecting an implementation.
- Define identity keys, device keys, device verification, forward secrecy, membership changes, key rotation, new-device enrollment and recovery.
- Consider encrypted attachments, local storage, multi-device synchronization and metadata leakage.
- Design server behavior so it stores and routes ciphertext without access to message decryption keys, where the selected protocol supports that model.
- Treat voice/media E2EE separately. WebRTC transport encryption through an SFU does not automatically mean the SFU cannot access media.
- Document product trade-offs such as search, moderation, reporting, push previews and account recovery.

Do not advertise RustChat as E2EE until the complete key lifecycle and relevant threat model have been implemented, tested and independently reviewed.

## 11. Audit logging and privacy

- Record security-relevant administrative actions and authentication events where appropriate.
- Do not log passwords, access tokens, private keys, encryption secrets or private message contents.
- Define retention and access controls for audit logs.
- Minimize personal data in telemetry and error reporting.

## 12. Threats to test

At minimum, consider:

- SQL injection and unsafe query construction.
- XSS and CSRF.
- IDOR/BOLA and privilege escalation.
- WebSocket hijacking and unauthorized subscriptions.
- Replay and duplicate operations.
- Malicious uploads and unsafe previews.
- SSRF and unsafe URL fetching.
- Brute force, spam and denial of service.
- Session theft and improper revocation.
- Secret leakage through logs, errors, backups or configuration.
- Incorrect E2EE device and group-key lifecycle behavior.

## 13. Vulnerability reporting

Before public release, provide a security contact and a clear private vulnerability-reporting process. Do not ask reporters to publish exploitable vulnerabilities before maintainers have had a reasonable opportunity to investigate and coordinate a fix.

## 14. Related documents

- [Architecture](ARCHITECTURE.md)
- [Database](DATABASE.md)
- [API and realtime protocol](API_AND_REALTIME.md)
