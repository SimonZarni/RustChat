# Authentication and Authorization Threat Model (initial)

## 1. Status

Initial Phase 0 draft, written before any authentication code exists. It must be reviewed and extended as design decisions in [SECURITY.md](SECURITY.md) and the PRD's open decisions are resolved.

## 2. Scope

Accounts, login, sessions, WebSocket authentication and resource authorization for the web and desktop clients. Out of scope for now: MFA, E2EE, voice tokens, bots and API tokens (each needs its own model).

## 3. Assets

- Account credentials (password hashes, reset tokens).
- Session identifiers and device records.
- Private message content and conversation membership.
- Community roles and permissions.
- Audit logs.

## 4. Trust boundaries

- Client (browser/desktop) to server: untrusted input and untrusted claims of identity.
- Server to PostgreSQL/Valkey: trusted network, least-privilege credentials.
- WebSocket connection to subscriptions: authorization checked per subscription and per event, not only at connect.

## 5. Threats and planned mitigations

| Threat | Mitigation (planned) | Test |
|---|---|---|
| Credential stuffing / brute force | Rate limits per account and IP; Argon2id cost tuned to deployment | Rate-limit and lockout behaviour tests |
| Account enumeration | Uniform login, registration and reset responses and timing | Compare responses for known/unknown accounts |
| Password database leak | Argon2id with per-password salt; no reversible storage | Hash format and parameter tests |
| Session theft | Random 256-bit IDs, HttpOnly/Secure/SameSite cookies, rotation on login and privilege change, expiry | Cookie attribute and rotation tests |
| Session not revoked | Server-side session store checked on each request; WebSocket closed or revalidated on revoke | Revoked session rejected over HTTP and WS |
| CSRF on cookie-authenticated requests | SameSite plus explicit CSRF token or origin check on state-changing requests | Cross-origin request rejected |
| WebSocket hijacking | Origin validation; no long-lived tokens in URLs | Disallowed origin refused |
| IDOR / BOLA | Every query scoped to the authenticated principal and resource membership | Per-endpoint denied-case tests |
| Privilege escalation via roles | Role hierarchy enforced server-side; cannot grant what you cannot delegate | Escalation and conflicting-override tests |
| Stale permissions on live connections | Permission change triggers subscription revalidation | Removed member stops receiving events |
| Reset token abuse | Short-lived, single-use, stored hashed | Reuse and expiry tests |
| Replay / duplicate requests | Idempotency keys with request fingerprint, enforced by DB constraints | Duplicate and conflicting-payload tests |
| Resource exhaustion | Bounded request sizes, connections, queues and rate limits | Oversize and flood tests |
| Secret leakage | No passwords, tokens or message contents in logs or errors | Log assertions in tests |

## 6. Implementation status

Updated after the accounts-and-sessions slice. "Tested" means an automated test exists and, for the starred rows, was mutation-checked (the control was deliberately broken and a test failed).

| Threat | Status |
|---|---|
| Credential stuffing / brute force | **Not mitigated.** Argon2id slows each guess, but there is no rate limiting or lockout yet. Required before exposing a server publicly |
| Account enumeration | Login: mitigated and tested (uniform response, dummy hash for unknown users)\*. Registration still reveals taken usernames; accepted for username-only accounts |
| Password database leak | Implemented and tested: Argon2id (19 MiB, t=2, p=1), per-hash salt |
| Session theft | Partly: random 256-bit token, only its SHA-256 stored, `HttpOnly`/`Secure`/`SameSite=Strict`/`__Host-` cookie, new token per login, 30-day absolute expiry. No idle timeout yet |
| Session not revoked | Implemented and tested for HTTP (logout, per-session revoke, expiry). WebSocket revalidation not applicable until WebSockets exist |
| CSRF | Implemented and tested: exact `Origin` allow-list on all state-changing requests, plus `SameSite=Strict`\* |
| WebSocket hijacking | Not applicable yet |
| IDOR / BOLA | Implemented and tested for sessions: revoke is scoped to the owner in the SQL statement itself and in the use case\* |
| Privilege escalation via roles | Not applicable yet |
| Stale permissions on live connections | Not applicable yet |
| Reset token abuse | Not applicable; password reset is not implemented |
| Replay / duplicate requests | Registration duplicates are rejected by a database constraint. Message idempotency is future work |
| Resource exhaustion | Partly: 16 KiB body limit, bounded DB pool, password length cap (128). No request rate limits yet |
| Secret leakage | Partly: passwords and tokens have redacted `Debug`, internal errors are logged server-side and never returned, database URL is redacted. Verified once by inspecting the server log for the password; there is no automated log-content test yet |

## 7. Open questions

- Desktop client authentication: the cookie model assumes the Tauri webview can hold the cookie; confirm when the desktop app is built.
- Exact rate-limit values and storage (Valkey versus in-process). This is the next security task.
- Email verification and password-reset policy (PRD open decisions); email is deferred.
