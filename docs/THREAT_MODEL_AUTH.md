# Authentication and Authorization Threat Model (initial)

## 1. Status

Living threat model, initially drafted during Phase 0 and updated as authentication and conversation features are implemented. Continue extending it as design decisions in [SECURITY.md](SECURITY.md) and the PRD's open decisions are resolved.

## 2. Scope

Accounts, login, sessions, WebSocket authentication and resource authorization for the web and desktop clients. Out of scope for now: MFA, E2EE, voice tokens, bots and API tokens (each needs its own model).

## 3. Assets

- Account credentials (password hashes, reset tokens).
- Session identifiers and device records.
- Private message content and conversation membership.
- Message reactions and their membership visibility.
- Community roles and permissions.
- Instance-admin assignment and account-suspension actions.
- Audit logs.

## 4. Trust boundaries

- Client (browser/desktop) to server: untrusted input and untrusted claims of identity.
- Server to PostgreSQL/Valkey: trusted network, least-privilege credentials.
- WebSocket connection to subscriptions: authorization checked per subscription and per event, not only at connect.

## 5. Threats and mitigations

| Threat | Mitigation (planned) | Test |
|---|---|---|
| Credential stuffing / brute force | Rate limits per account and IP; Argon2id cost tuned to deployment | Rate-limit and lockout behaviour tests |
| Account enumeration | Uniform login, registration and reset responses and timing | Compare responses for known/unknown accounts |
| Password database leak | Argon2id with per-password salt; no reversible storage | Hash format and parameter tests |
| Session theft | Random 256-bit IDs, HttpOnly/Secure/SameSite cookies, a new token on every login, revocation and expiry | Cookie attribute, new-login token, and revocation tests |
| Session not revoked | Server-side session store checked on each request; WebSocket closed or revalidated on revoke | Revoked session rejected over HTTP and WS |
| CSRF on cookie-authenticated requests | SameSite plus explicit CSRF token or origin check on state-changing requests | Cross-origin request rejected |
| WebSocket hijacking | Origin validation; no long-lived tokens in URLs | Disallowed origin refused |
| IDOR / BOLA | Every query scoped to the authenticated principal and resource membership | Per-endpoint denied-case tests |
| Unauthorized reaction access | Read and mutation endpoints verify conversation membership and channel visibility; database constraints bind each reaction to its message, conversation and member | Reaction idempotency and outsider-denial API tests |
| Forged delivery acknowledgement | Acknowledgement is bound to the authenticated member and conversation; sequence cannot exceed the server's latest sequence and the stored cursor only advances | Monotonicity, bounds and outsider-denial API tests |
| Cross-conversation reply reference | Reply targets are checked against the sending conversation; a composite database foreign key also binds target and reply to the same conversation | API test for foreign targets, durable reply history and idempotency conflicts |
| Mention leaks unrelated accounts | Mention resolution searches only current conversation members; membership-scoped foreign keys constrain persisted targets | API tests verify members resolve, outsiders do not, and edits refresh the set |
| Profile enumeration or privacy bypass | Exact username lookup enforces discoverability and profile visibility; direct/group creation uses the same discoverability rule, with existing conversation membership as the only hidden-user exception | API tests cover unknown/unlisted users, hidden profile access, and existing conversation-member access |
| Privilege escalation via roles | Role hierarchy enforced server-side; cannot grant what you cannot delegate | Escalation and conflicting-override tests |
| Unauthorized instance administration | Instance authority comes only from configured account UUIDs; self-suspension and changes to another configured admin are rejected; actual changes revoke sessions and are audited | Admin API integration covers non-admin denial, self/admin target conflicts, session revocation and audit records |
| Stale permissions on live connections | Permission change triggers subscription revalidation | Removed member stops receiving events |
| Reset token abuse | One-hour, single-use tokens stored as SHA-256 digests | Reuse and expiry tests |
| Replay / duplicate requests | Idempotency keys with request fingerprint, enforced by DB constraints | Duplicate and conflicting-payload tests |
| Resource exhaustion | Bounded request sizes, connections, queues and rate limits | Oversize and flood tests |
| Secret leakage | No passwords, tokens or message contents in logs or errors | Log assertions in tests |

## 6. Implementation status

Updated through community roles, message editing/deletion/replies/mentions/reactions, password changes, account lifecycle endpoints, shared authentication rate limits, instance administration, delivery acknowledgements, WebSocket delivery and slow-reader handling, and the initial web client. "Tested" means an automated test exists and, for the starred rows, was mutation-checked (the control was deliberately broken and a test failed).

| Threat | Status |
|---|---|
| Credential stuffing / brute force | Mitigated and tested\*: per-network limit on all login attempts, per-username limit on failures (applies to unknown names too), per-network registration limit, all checked before hashing. The production limiter stores only SHA-256 key digests in PostgreSQL, so instances sharing the database share budgets. At the current default thresholds, the per-username budget is 10 failed guesses per 15 minutes; concurrent requests can pass a check before either records its failure, so this is not a strict atomic ceiling. The same budget lets an attacker deliberately keep a victim's logins blocked for up to 15 minutes (accepted trade-off; the victim's existing sessions are unaffected). Operators can configure the thresholds. Database unavailability also prevents authentication rate-limit checks and fails these requests closed. |
| Account enumeration | Login: mitigated and tested (uniform response, dummy hash for unknown users)\*. Registration still reveals taken usernames; accepted for username-only accounts |
| Password database leak | Implemented and tested: Argon2id (19 MiB, t=2, p=1), per-hash salt |
| Session theft | Implemented and tested: random 256-bit token, only its SHA-256 stored, `HttpOnly`/`Secure`/`SameSite=Strict`/`__Host-` cookie, new token per login, 30-day absolute expiry, configurable idle timeout (default 7 days), and token rotation every 24 hours with a 60-second grace period for the replaced token (a copied cookie only keeps working while its holder stays in step with rotation: once one party presents the old token after a rotation, the other copy stops working after the grace period, which also makes a stolen session visible as an unexpected sign-out). The replaced token can be used for up to a minute after rotation; this is accepted so parallel requests are not signed out |
| Session not revoked | Implemented and tested for HTTP (logout, per-session revoke, expiry); WebSocket sessions are revalidated every 30 seconds |
| CSRF | Implemented and tested: exact `Origin` allow-list on all state-changing requests, plus `SameSite=Strict`\* |
| WebSocket hijacking | Handshake requires the session cookie and an exact configured `Origin`; tokens are not placed in URLs. PostgreSQL-backed integration tests cover allowed/denied origins and authenticated socket behavior |
| IDOR / BOLA | Implemented and API-tested for sessions, conversations, member profiles, messages, communities, channels, and role operations. Access is membership/permission scoped; hidden resources return `404` |
| Profile enumeration or privacy bypass | Implemented and tested: discoverability gates exact lookup and new conversation creation; conversation-only profiles are available to existing conversation members; hidden profiles return `404` |
| Presence disclosure outside a shared conversation or against user settings | Implemented: server checks shared conversation membership and `presence_visible` before sending online/offline updates; privacy changes clear or refresh the recipient state. Covered by WebSocket integration tests |
| Unauthorized reaction access | Implemented and API-tested: reaction lists and writes require membership and channel read permission; outsider requests return `404`; composite database foreign keys bind reactions to the message and current member |
| Forged delivery acknowledgement | Implemented and API-tested: only conversation members can advance delivery state, and sequence bounds plus monotonic updates are enforced in PostgreSQL transactions |
| Privilege escalation via roles | Role hierarchy and delegated permission checks are implemented and API-tested, including channel overrides and owner-only lifecycle operations |
| Stale permissions on live connections | Subscriptions and permissions are rechecked every 30 seconds and removed when access disappears. A revoked member may receive events during that interval |
| Reset token abuse | Password reset uses one-hour random tokens stored as SHA-256 digests; successful use is atomic and single-use, and revokes all sessions. API coverage verifies replay and expiry. SMTP failures are logged without exposing account existence in the response; provider-side delivery remains an operational dependency |
| Replay / duplicate requests | Tested API behavior: message retry with the same sender, conversation, client ID and content returns the original record; changed content conflicts. Message insertion and outbox event are transactional. Socket event delivery is at least once; clients deduplicate and recover by synchronizing message history |
| Resource exhaustion | Partly: 16 KiB body and WebSocket frame limits, bounded DB pool, password length cap (128), message size cap (4,000 chars / 16,000 bytes), expiring PostgreSQL-backed authentication counters keyed by SHA-256 digests, 512-event process bus, 100 subscriptions per connection, group member cap (configurable 2–500), page size cap (100), limited requests skip hashing, and a five-second socket-write timeout. per-user budgets by request class (reads, writes, message sends, creations) and a per-network budget for failed-authentication requests on all authenticated API routes, kept per server process. These do not replace network-level protection: a proxy or firewall limit in front is still advisable for volumetric floods, and the WebSocket upgrade, health and metrics routes are outside the API budgets |
| Client IP spoofing to evade limits | Implemented and tested\*: `X-Forwarded-For` is ignored unless trusted proxy hops are configured, then only the entry the trusted proxy appended is used; missing connection info fails closed. Misconfiguration (hops set while the server is directly reachable) lets clients choose their own address, so it is documented as proxy-only |
| Secret leakage | Partly: passwords and tokens have redacted `Debug`, internal errors are logged server-side and never returned, database URL is redacted. Verified once by inspecting the server log for the password; there is no automated log-content test yet |

## 7. Open questions

- Desktop client authentication: the cookie model assumes the Tauri webview can hold the cookie; confirm when the desktop app is built.
- Authentication rate-limit thresholds are configurable, but their windows are fixed. The production limiter uses PostgreSQL and fails closed when the database is unavailable. WebSocket event fan-out and presence remain process-local; multi-instance deployments need shared realtime coordination.
- Password reset uses the verified account recovery email and requires SMTP configuration. Operators must tell users who registered an address before verification existed to confirm it in their settings.
