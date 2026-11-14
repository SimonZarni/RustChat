# 0004. Server-side sessions in HttpOnly cookies

- **Status:** Accepted
- **Date:** 2026-11-14

## Context

Browsers and the Tauri desktop app need an authentication mechanism that resists XSS token theft, supports immediate revocation and per-device session management (PRD 4.1), and does not require clients to handle secrets.

## Decision

Authenticate with an opaque random 256-bit session token carried in a cookie. The cookie is `HttpOnly`, `Secure`, `SameSite=Strict`, `Path=/`, and uses the `__Host-` prefix when TLS is on. Only the SHA-256 digest of the token is stored, in a `sessions` row that can be revoked or expire (30 days absolute). A new token is issued on every login. State-changing requests must carry an `Origin` header matching an operator-configured allow-list (CSRF defence).

## Alternatives considered

- **Bearer tokens kept in JavaScript:** readable by any XSS, so rejected for the web client.
- **Stateless JWTs:** cannot be revoked without a server-side list, which removes the main benefit; larger and easy to misuse.
- **Storing the raw token server-side:** a database leak would hand out live sessions; hashing costs nothing.

## Consequences

Revocation and session listing are straightforward and tested. Every authenticated request reads the database (acceptable now; a cache is possible later). Non-browser clients such as bots will need a separate token type (PRD 4.16). The desktop client is assumed to keep cookies in its webview; confirm when it is built. WebSocket authentication is not designed yet and must reuse this mechanism without placing tokens in URLs.
