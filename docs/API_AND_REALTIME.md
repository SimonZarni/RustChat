# RustChat API and Realtime Protocol

## 1. Purpose and status

This document defines the intended communication conventions between RustChat clients and the backend. Endpoint paths and event names below are illustrative until implemented and published in the API contract.

Use REST over HTTPS for request/response operations and WebSockets over WSS for realtime server-pushed events.

## 2. General API conventions

- Prefix APIs with `/api/v1`.
- Use JSON for ordinary request and response bodies.
- Use consistent resource naming and HTTP status codes.
- Validate all input on the server.
- Authenticate and authorize every protected operation.
- Return structured, non-sensitive error responses.
- Apply request body, pagination and rate limits.
- Document the contract using OpenAPI and keep it synchronized with implementation.

Illustrative endpoints:

```http
POST   /api/v1/auth/register
POST   /api/v1/auth/login
POST   /api/v1/auth/logout

GET    /api/v1/conversations
POST   /api/v1/conversations
GET    /api/v1/conversations/{conversation_id}/messages
POST   /api/v1/conversations/{conversation_id}/messages

GET    /api/v1/servers
POST   /api/v1/servers
POST   /api/v1/servers/{server_id}/channels
```

These paths are examples, not a finalized endpoint inventory.

### Implemented: accounts and sessions

These endpoints exist (`backend/crates/http`). All other endpoints above remain illustrative. Registration and login are by username and password only; email, verification and password reset are not implemented.

| Method and path | Auth | Success | Notes |
|---|---|---|---|
| `POST /api/v1/auth/register` | none | `201 {"user": {...}}` | Body `{"username","password"}`. Does not log the user in. `403 REGISTRATION_CLOSED` when `RUSTCHAT_REGISTRATION=closed` |
| `POST /api/v1/auth/login` | none | `200 {"user": {...}}` | Sets the session cookie. A new session and token are issued on every login |
| `POST /api/v1/auth/logout` | cookie | `204` | Revokes the current session and clears the cookie |
| `GET /api/v1/auth/me` | cookie | `200 {"user": {...}}` | |
| `GET /api/v1/auth/sessions` | cookie | `200 {"sessions": [{id, created_at, expires_at, current}]}` | Own live sessions only (max 100) |
| `DELETE /api/v1/auth/sessions/{id}` | cookie | `204` | Own sessions only; another user's or unknown id is `404` |

`user` is `{id, username, created_at}`. Times are RFC 3339 UTC. `GET /healthz` returns `{"status":"ok"}` outside the `/api/v1` prefix.

**Constraints and rules**

- Usernames: 3 to 32 characters, `[A-Za-z0-9_]`, case-insensitive. Passwords: 12 to 128 characters.
- Bodies must be `application/json`, at most 16 KiB, with no unknown fields.
- Session cookie: `__Host-rustchat_session` (`rustchat_session` when `RUSTCHAT_COOKIE_SECURE=false`, local development only). `HttpOnly`, `Secure`, `SameSite=Strict`, `Path=/`, `Max-Age` equal to the session lifetime. The value is a 256-bit random token; only its SHA-256 is stored server-side.
- CSRF: every non-`GET`/`HEAD`/`OPTIONS` request under `/api/v1` must carry an `Origin` header exactly matching an entry in `RUSTCHAT_ALLOWED_ORIGINS`, otherwise `403 ORIGIN_NOT_ALLOWED`. This includes login and register.
- Responses under `/api/v1` carry `Cache-Control: no-store`.
- Failed login is the same `401 INVALID_CREDENTIALS` whether the account exists, the username is malformed or the password is wrong, and unknown accounts still pay the hashing cost. Registration necessarily reveals whether a username is taken (`409 USERNAME_TAKEN`).

**Error codes in use:** `VALIDATION_ERROR` (422), `USERNAME_TAKEN` (409), `REGISTRATION_CLOSED` (403), `INVALID_CREDENTIALS` (401), `UNAUTHENTICATED` (401), `ORIGIN_NOT_ALLOWED` (403), `BAD_REQUEST` (400/415/422), `PAYLOAD_TOO_LARGE` (413), `NOT_FOUND` (404), `INTERNAL` (500).

**Not yet implemented:** rate limiting and lockout on authentication endpoints, session idle timeout and rotation, WebSocket authentication, password change and reset, MFA.

## 3. Request and response conventions

Use explicit request and response types. Avoid exposing database models directly as public API contracts.

A successful response should provide the resource or operation result needed by the client. Errors should use a stable machine-readable code and a safe human-readable message.

Example error:

```json
{
  "error": {
    "code": "FORBIDDEN",
    "message": "You do not have permission to perform this action."
  }
}
```

Do not expose stack traces, SQL errors, internal paths, secrets or sensitive authorization details.

## 4. Authentication and WebSockets

WebSocket connections must be authenticated using a deliberate mechanism compatible with the web and desktop clients. Avoid putting long-lived bearer tokens in URLs, where they may leak through logs and diagnostics.

The server must:

- validate the authenticated identity during connection setup,
- validate origin and deployment policy for browser clients,
- enforce connection and message-size limits,
- authorize subscriptions and actions,
- close or revalidate connections when sessions are revoked,
- avoid trusting client-supplied user IDs.

Use TLS for all production HTTP and WebSocket traffic.

## 5. WebSocket lifecycle

Conceptual lifecycle:

1. Client obtains an authenticated session.
2. Client opens a WSS connection.
3. Server authenticates the connection.
4. Client subscribes or synchronizes only to resources it is authorized to access.
5. Server sends events and acknowledgements.
6. Client handles disconnects with bounded exponential backoff and jitter.
7. Client reconnects and synchronizes from its last durable cursor.

The server must enforce limits for connections, subscriptions, inbound message sizes, event rates and queued outbound data. Slow clients must not create unbounded memory growth.

## 6. Event envelope

Use a versioned event envelope with a unique event ID and enough routing/order metadata for safe processing.

Illustrative event:

```json
{
  "type": "message.created",
  "version": 1,
  "event_id": "019...",
  "conversation_id": "019...",
  "sequence": 1024,
  "occurred_at": "2026-01-01T12:00:00Z",
  "payload": {
    "message_id": "019...",
    "sender_id": "019...",
    "content": "Hello RustChat!"
  }
}
```

The actual payload will depend on encryption mode and the final message schema. Do not assume plaintext content will be present once E2EE is implemented.

## 7. Event naming

Use stable, namespaced event names. Initial candidates:

- `message.created`
- `message.updated`
- `message.deleted`
- `reaction.added`
- `reaction.removed`
- `typing.started`
- `typing.stopped`
- `presence.updated`
- `conversation.read`
- `channel.created`
- `member.joined`
- `member.removed`

Finalize event names and payload schemas as each feature is implemented.

## 8. Message sending and idempotency

The client generates a stable UUIDv7 client message ID before sending.

The server should:

1. Authenticate the sender.
2. Verify access to the target conversation/channel.
3. Validate the payload and size.
4. Check the idempotency key and request fingerprint.
5. In one transaction, allocate sequence, persist the message and persist its outbox event.
6. Return the canonical message result and sequence.

The same key and same request should resolve to the same logical operation. The same key with a different request must return a conflict. Enforce this with database constraints, not only an in-memory check.

## 9. Acknowledgements and delivery semantics

Distinguish these states:

- **Accepted/persisted:** the server committed the message.
- **Published:** an event publisher processed the outbox record.
- **Received by client:** a connected client received the event.
- **Read:** a user/client advanced its read cursor.

Do not conflate server persistence with recipient delivery or reading.

The system should support at-least-once event delivery and idempotent client processing. Exactly-once delivery over an unreliable network is not promised.

## 10. Reconnection and synchronization

Realtime events are an optimization for low-latency updates, not the only source of truth.

After reconnecting, the client should request missed changes from a durable cursor. The backend checks current authorization before returning data. Clients deduplicate events by event ID or resource identity and apply sequence ordering.

Define behavior for expired cursors, deleted resources, revoked membership and events that are no longer visible to the user.

## 11. Valkey and event distribution

Valkey Pub/Sub may be used initially for ephemeral distribution between application instances. It is not durable and must not be the sole record of a business event.

The PostgreSQL outbox provides recoverability. If durable cross-instance streaming becomes necessary, evaluate a broker such as NATS JetStream without moving business rules into broker-specific code.

## 12. Compatibility and versioning

- Version externally visible APIs and event envelopes.
- Prefer additive changes where possible.
- Define how old clients handle unknown event types or fields.
- Do not silently change the meaning of an existing field.
- Update tests and documentation whenever a contract changes.

## 13. Related documents

- [Architecture](ARCHITECTURE.md)
- [Database](DATABASE.md)
- [Security](SECURITY.md)
