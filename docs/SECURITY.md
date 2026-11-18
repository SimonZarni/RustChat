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
- TOTP two-step verification is implemented (optional, enabled by a server key); WebAuthn remains a candidate for later.
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

- The implemented upload flow enforces a configurable per-file limit (25 MiB by default, maximum 100 MiB) and per-user ready-file quota (2 GiB by default). The API checks conversation membership and message association; download URLs are issued only to current members.
- The client MIME type is checked against a supported set and content signatures are inspected on completion. Files are stored under generated opaque keys, never their supplied filenames. Downloads are forced as attachments with `application/octet-stream`; inline preview and malware scanning are not implemented.
- Upload and download URLs are short-lived bearer credentials. Uploads first target a staging key; completion checks the object size and signature before promoting it to a final key. A browser may still use an issued staging URL until it expires, even after the client cancels. The server claims expired pending rows every minute, deletes staging and final objects, and purges metadata only after both deletes succeed. Failed deletions are retried; configure object-store lifecycle cleanup for the staging prefix as a secondary safeguard.
- Keep the S3-compatible bucket private, limit credentials to the application bucket, and configure CORS only for the exact RustChat web origin and required `PUT`/`GET` methods and headers. The signed object-store endpoint is browser-facing and must use HTTPS in secure-cookie deployments.
- Object storage is separate from PostgreSQL. `backup-full.ps1` and `restore-full.ps1` stop the server and coordinate bucket and database snapshots. Restore requires an empty bucket and leaves the API stopped if either half fails. The S3 copy/restore flow was checked against local Garage; production-provider recovery still requires an operator rehearsal.

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
- Notification records and realtime hints omit message text; inbox reads are scoped to the recipient and notifications from blocked users are hidden.
- Per-conversation notification settings are readable and writable only through the caller's current conversation/channel visibility; message and mention preferences are enforced in the message transaction and do not suppress message delivery.

## 12. Threats to test

At minimum, consider:

- SQL injection and unsafe query construction.
- XSS and CSRF.
- IDOR/BOLA and privilege escalation.
- Friend request ownership and block bypass through profile, presence, direct-message, and group-member entry points.
- WebSocket hijacking and unauthorized subscriptions.
- Replay and duplicate operations.
- Malicious uploads and unsafe previews.
- SSRF and unsafe URL fetching.
- Brute force, spam and denial of service.
- Session theft and improper revocation.
- Secret leakage through logs, errors, backups or configuration.
- Incorrect E2EE device and group-key lifecycle behavior.

## 13. Vulnerability reporting

Private vulnerability reporting is enabled through GitHub; the process and expectations are in [.github/SECURITY.md](../.github/SECURITY.md). Do not ask reporters to publish exploitable vulnerabilities before maintainers have had a reasonable opportunity to investigate and coordinate a fix. Before a stable release, add a monitored contact that does not depend on one GitHub account and define response targets.

## 14. Related documents

- [Architecture](ARCHITECTURE.md)
- [Database](DATABASE.md)
- [API and realtime protocol](API_AND_REALTIME.md)
