# RustChat API and Realtime Protocol

## 1. Purpose and status

This document records the current HTTP and WebSocket contract. Sections marked illustrative or planned describe future work; implemented endpoints are listed explicitly.

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
GET    /api/v1/realtime   (WebSocket upgrade)

GET    /api/v1/conversations
POST   /api/v1/conversations
GET    /api/v1/conversations/{conversation_id}/messages
POST   /api/v1/conversations/{conversation_id}/messages

GET    /api/v1/servers
POST   /api/v1/servers
POST   /api/v1/servers/{server_id}/channels
```

The examples above mix implemented and planned paths; the tables below are authoritative for implemented routes. `/servers` is planned terminology; the implemented resource is `/communities`.

### Implemented: accounts and sessions

These endpoints exist (`backend/crates/http`). Conversations and membership are also implemented below; the remaining endpoints above are illustrative. Registration and login use a username and password. Registration can include an optional recovery email; email verification is deferred.

### Implemented: instance administration

Instance administrators are assigned through the comma-separated `RUSTCHAT_ADMIN_USER_IDS` environment variable. The value is a list of account UUIDs; community roles do not grant instance-wide authority. An administrator can list the newest 100 accounts and suspend or restore a non-admin account. Suspension immediately revokes active sessions and prevents login. Administrators cannot suspend themselves or another configured administrator. Each actual state change is recorded in `instance_admin_audit_log`; repeated requests that leave the state unchanged do not add duplicate audit rows.

| Method and path | Auth | Success | Notes |
|---|---|---|---|
| `GET /api/v1/admin/users` | configured instance admin | `200 {"users":[...]}` | Newest 100 accounts; response omits email and credential data |
| `PUT /api/v1/admin/users/{user_id}/suspension` | configured instance admin | `200 {"user":...}` | Body `{"suspended":true}` or `false`; self/admin targets return `409`; non-admin callers receive `403` |

| Method and path | Auth | Success | Notes |
|---|---|---|---|
| `POST /api/v1/auth/register` | none | `201 {"user": {...}}` | Body `{"username":"...","password":"...","email":"..."}`; email is optional and used for password recovery. Verification is deferred. Does not log the user in. `403 REGISTRATION_CLOSED` when `RUSTCHAT_REGISTRATION=closed` |
| `POST /api/v1/auth/login` | none | `200 {"user": {...}}` | Sets the session cookie. A new session and token are issued on every login |
| `POST /api/v1/auth/logout` | cookie | `204` | Revokes the current session and clears the cookie |
| `GET /api/v1/auth/me` | cookie | `200 {"user": {...}}` | |
| `GET /api/v1/auth/profile` | cookie | `200 {"profile": {...}}` | Current user's profile (`id`, `username`, `display_name`, `bio`, `updated_at`) |
| `PATCH /api/v1/auth/profile` | cookie | `200 {"profile": {...}}` | Body `{"display_name":"...","bio":"..."}`; display name up to 64 characters, bio up to 280 |
| `GET /api/v1/auth/privacy` | cookie | `200 {"privacy": {...}}` | Read own discoverability, profile visibility, and presence visibility settings |
| `PATCH /api/v1/auth/privacy` | cookie + allowed `Origin` | `200 {"privacy": {...}}` | Body `{"discoverable":bool,"profile_visibility":"everyone"|"conversation_members","presence_visible":bool}`; `presence_visible` defaults to true when omitted and presence is visible only to users who share a conversation |
| `PATCH /api/v1/auth/username` | cookie + allowed `Origin` | `200 {"user": {...}}` | Body `{"username":"..."}`; normalized case-insensitively and subject to the registration username rules; conflicts return `409 USERNAME_TAKEN` |
| `GET /api/v1/users/{username}/profile` | cookie | `200 {"profile": {...}}` | Exact username lookup; `404` when undiscoverable or private to another user; existing conversation members can view a conversation-only profile |
| `GET /api/v1/auth/sessions` | cookie | `200 {"sessions": [{id, created_at, expires_at, current}]}` | Own live sessions only (max 100) |
| `DELETE /api/v1/auth/sessions/{id}` | cookie | `204` | Own sessions only; another user's or unknown id is `404` |
| `POST /api/v1/auth/password` | cookie + allowed `Origin` | `204` | Body: `current_password`, `new_password`; wrong current password returns generic `INVALID_CREDENTIALS`; keeps the current session and revokes other sessions atomically with the password update |
| `POST /api/v1/auth/password-reset/request` | none | `202` | Body `{"email":"..."}`. Returns the same response for unknown and eligible addresses; requires SMTP configuration. Reset tokens expire after one hour and only token digests are stored |
| `POST /api/v1/auth/password-reset/complete` | none | `204` | Body `{"token":"...","new_password":"..."}`. Tokens are single-use; successful reset revokes all existing sessions |
| `POST /api/v1/auth/deactivate` | cookie + allowed `Origin` | `204` | Body: `{"current_password":"..."}`; revokes all sessions and clears the cookie. A later successful password login reactivates the account. Community owners must transfer ownership first (`409 ACCOUNT_OWNS_COMMUNITIES`). Current-password checks use the login IP and username rate limits |
| `POST /api/v1/auth/delete` | cookie + allowed `Origin` | `204` | Body: `{"current_password":"...","confirm":true}`; permanently tombstones the account, clears profile details, removes memberships, and revokes sessions. Messages and audit references are retained with the account's user ID; community ownership must first be transferred. Historical backups can retain pre-deletion data until their configured retention period expires |

`user` is `{id, username, created_at}`. `profile` is `{id, username, display_name, bio, updated_at}`. Times are RFC 3339 UTC. `GET /healthz` is a process liveness check and returns `{"status":"ok"}`. `GET /readyz` checks PostgreSQL connectivity and returns `{"status":"ok"}` with `200`, or `{"status":"unavailable"}` with `503`. Both are outside the `/api/v1` prefix. `GET /metrics` returns low-cardinality process-local HTTP response counters in Prometheus text format; it is available only on the private server network and Caddy returns `404` for external requests.

Profile lookup is exact by username. When `discoverable` is false, unrelated users cannot resolve the account or start a new direct/group conversation with it by username. `profile_visibility=conversation_members` restricts profile details to the owner and users who already share a conversation; usernames still appear to current conversation members for messaging. Privacy checks are enforced on the server.

**Constraints and rules**

- Usernames: 3 to 32 characters, `[A-Za-z0-9_]`, case-insensitive. Passwords: 12 to 128 characters.
- Bodies must be `application/json`, at most 16 KiB, with no unknown fields.
- Session cookie: `__Host-rustchat_session` (`rustchat_session` when `RUSTCHAT_COOKIE_SECURE=false`, local development only). `HttpOnly`, `Secure`, `SameSite=Strict`, `Path=/`, `Max-Age` equal to the session lifetime. The value is a 256-bit random token; only its SHA-256 is stored server-side. Sessions end after `RUSTCHAT_SESSION_IDLE_TIMEOUT_DAYS` (default 7) without activity, in addition to the 30-day absolute limit; activity is recorded at most every five minutes. The token is rotated at most once every 24 hours: the response to the first authenticated request after that carries a replacement `Set-Cookie`, and the previous token keeps working for 60 seconds so requests already in flight are not signed out (the grace period never triggers another rotation). The WebSocket session re-check enforces the idle timeout but does not record activity or rotate.
- CSRF: every non-`GET`/`HEAD`/`OPTIONS` request under `/api/v1` must carry an `Origin` header exactly matching an entry in `RUSTCHAT_ALLOWED_ORIGINS`, otherwise `403 ORIGIN_NOT_ALLOWED`. This includes login and register.
- Responses under `/api/v1` carry `Cache-Control: no-store`.
- Rate limiting (see below): `429 RATE_LIMITED` with a `Retry-After` header (whole seconds).
- Failed login is the same `401 INVALID_CREDENTIALS` whether the account exists, the username is malformed or the password is wrong, and unknown accounts still pay the hashing cost. Registration necessarily reveals whether a username is taken (`409 USERNAME_TAKEN`).

**Rate limits** (fixed windows, shared through PostgreSQL in the server deployment):

| Limit | Counts | Default |
|---|---|---|
| Login per client network | every attempt, any outcome | 30 per 15 minutes |
| Login per username | failed attempts only; a successful login clears it | 10 per 15 minutes |
| Register per client network | every attempt, any outcome | 5 per hour |

Set login and registration caps with `RUSTCHAT_LOGIN_PER_IP_LIMIT`, `RUSTCHAT_LOGIN_PER_USERNAME_LIMIT`, and `RUSTCHAT_REGISTER_PER_IP_LIMIT` (each 1–100000); their time windows remain 15 minutes, 15 minutes, and one hour. `RUSTCHAT_GROUP_MEMBER_LIMIT` sets the group cap (2–500), and `RUSTCHAT_DB_MAX_CONNECTIONS` sets the PostgreSQL pool size. Limits are checked before any password hashing, so a limited request costs almost nothing. Usernames are case-folded and counted whether or not the account exists, so a `429` never reveals which accounts exist. A client network is the IPv4 address or the IPv6 /64 prefix. By default it is the TCP peer address and `X-Forwarded-For` is ignored; set `RUSTCHAT_TRUSTED_PROXY_HOPS=N` only when the server is reachable exclusively through N reverse proxies, in which case the entry N places from the right of `X-Forwarded-For` is used. If that header is missing or malformed the peer address is used. Other endpoints are not rate limited yet.

**Error codes in use:** `VALIDATION_ERROR` (422), `USERNAME_TAKEN` (409), `REGISTRATION_CLOSED` (403), `INVALID_CREDENTIALS` (401), `UNAUTHENTICATED` (401), `ORIGIN_NOT_ALLOWED` (403), `BAD_REQUEST` (400/415/422), `PAYLOAD_TOO_LARGE` (413), `RATE_LIMITED` (429), `NOT_FOUND` (404), `EMAIL_UNAVAILABLE` (503), `INVALID_ACCOUNT_TOKEN` (400), `INVALID_CONVERSATION` (422), `MEMBER_LIMIT` (422), `IDEMPOTENCY_CONFLICT` (409), `INVALID_REACTION` (422), `INVALID_REACTION_REQUEST` (400), `INVALID_DELIVERY_CURSOR` (422), `INTERNAL` (500).

**Not yet implemented:** email verification, MFA. Authentication rate-limit state is shared through PostgreSQL, with hashed keys and expiry. Password reset reuses the login IP and per-username rate limit budgets. Password change reuses login IP and per-username failure limits. Multi-instance WebSocket event fan-out is not implemented.

### Implemented: conversations and membership

All routes require the session cookie. Mutating routes also require an allowed `Origin`.

| Method and path | Success | Notes |
|---|---|---|
| `POST /api/v1/conversations` | `201 {"conversation": {...}}` | Body is `{"type":"direct","username":"bobby"}` or `{"type":"group","usernames":["bobby","carol"]}`. Group creator is included automatically. Repeated direct creation returns the same conversation |
| `GET /api/v1/conversations?before={id}&limit=50` | `200 {"conversations":[...],"next_cursor":...}` | Caller’s conversations only; default limit 50, maximum 100 |
| `GET /api/v1/conversations/{id}` | `200 {"conversation": {...}}` | Non-members and unknown IDs both receive `404` |
| `GET /api/v1/conversations/{id}/members` | `200 {"members":[{"id","username","created_at"}]}` | Profile details are returned only to current conversation members; unknown conversations and non-members both receive `404` |
| `POST /api/v1/conversations/{id}/members` | `204` | Body `{"username":"david"}`. Existing members can add members to groups |
| `DELETE /api/v1/conversations/{id}/members/{user_id}` | `204` | Existing group members can remove any group member, including themselves. Direct membership is fixed |

`conversation` contains `{id, kind, created_by, created_at, members}`. `kind` is `direct` or `group`; timestamps are RFC 3339 UTC. Exact username matching follows the existing case-insensitive username rules. Unknown usernames return `404`. The group member cap defaults to 100 including the creator, is configured by `RUSTCHAT_GROUP_MEMBER_LIMIT`, and accepts values 2–500. Adding a current member is idempotent. Rejected group sizes return `422 MEMBER_LIMIT`; invalid group operations return `422 INVALID_CONVERSATION`.

Every read and membership mutation is scoped to caller membership in the database. Any group member can manage group membership. Community channel operations additionally enforce role permissions. Conversation list cursors use the last conversation ID from the prior page; ordering is `(created_at, id)` descending.

The member profile response is the supported way for a conversation participant to resolve member IDs to usernames. It does not provide a global user directory.

### Implemented: communities, channels and roles

| Method and path | Success | Notes |
|---|---|---|
| `POST /api/v1/communities` / `GET /api/v1/communities` | `201` / `200` | Create a community; list caller memberships |
| `GET /api/v1/communities/{id}/members` | `200` | Members visible to current community members |
| `DELETE /api/v1/communities/{id}/members` | `204` | Leave; owner must transfer ownership first |
| `DELETE /api/v1/communities/{id}/members/{user_id}` | `204` | Remove another member with `MANAGE_MEMBERS`; owner and self-removal are denied |
| `POST /api/v1/communities/{id}/invites` | `201` | Optional `ttl_seconds`, `max_uses`; invite codes are returned once and stored as digests |
| `GET /api/v1/communities/{id}/invites` | `200` | Lists invitation metadata for callers with invite-management permission; token material is never returned |
| `DELETE /api/v1/communities/{id}/invites` | `204` | Body `{"invite_code":"..."}`; caller needs invite-management permission |
| `DELETE /api/v1/communities/{id}/invites/{invite_id}` | `204` | Revokes a listed invitation by ID; caller needs invite-management permission |
| `POST /api/v1/communities/join` | `200` | Body `{"invite_code":"..."}`; checks expiry, revocation and use limit |
| `GET` / `POST /api/v1/communities/{id}/channels` | `200` / `201` | List visible or create a text channel; POST accepts name, description, read_only and optional category_id |
| `PATCH` / `DELETE /api/v1/communities/{id}/channels/{channel_id}` | `200` / `204` | Rename, describe, set read-only, reorder, change category_id, or delete a channel |
| `GET /api/v1/communities/{id}/categories` | `200` | List ordered categories for a community member |
| `POST /api/v1/communities/{id}/categories` | `201` | Create category; requires `MANAGE_CHANNELS` |
| `PATCH` / `DELETE /api/v1/communities/{id}/categories/{category_id}` | `200` / `204` | Rename/reorder or delete; deletion leaves assigned channels uncategorized |
| `GET` / `POST /api/v1/communities/{id}/roles` | `200` / `201` | List or create roles with delegated permissions |
| `PATCH` / `DELETE /api/v1/communities/{id}/roles/{role_id}` | `200` / `204` | Edit/reorder or delete a custom role; default role cannot be deleted |
| `POST` / `DELETE /api/v1/communities/{id}/members/{user_id}/roles/{role_id}` | `204` | Assign/remove a role subject to hierarchy |
| `PUT /api/v1/channels/{channel_id}/permissions/{role_id}` | `204` | Set channel allow/deny permission bits |
| `PATCH /api/v1/communities/{id}` | `200` | Update community settings; owner only |
| `POST /api/v1/communities/{id}/ownership` | `204` | Transfer ownership to an existing member |
| `DELETE /api/v1/communities/{id}` | `204` | Delete community with explicit name confirmation; audit record is retained |

The current permission evaluator combines member role grants, then channel allows, then channel denies (deny wins). A permission bit cannot be both allowed and denied in one override. Custom roles are ordered from highest rank (position 1) to lowest; delegated managers can edit, delete, assign, or remove only roles below their highest assigned custom role, cannot move a role to their rank or above, and cannot grant permissions they do not hold. Community owners can manage all roles. Community lifecycle and membership/role/channel mutations are audited. Categories can be created, renamed, reordered and deleted in the web client; channels can be assigned or moved between categories. The web client does not yet expose every administrative API.

### Implemented: plain-text messages

Both routes require the session cookie. Sending also requires an allowed `Origin`.

| Method and path | Success | Notes |
|---|---|---|
| `POST /api/v1/conversations/{id}/messages` | `201 {"message": {...}}` | Body `{"client_message_id":"<UUIDv7>","content":"Hello","reply_to_message_id":"<optional message UUID>"}`. Reply targets must belong to this conversation. A retry with the same ID and same content/reply target returns the original message; changing either returns `409 IDEMPOTENCY_CONFLICT` |
| `GET /api/v1/conversations/{id}/messages?before_sequence=42&limit=50` | `200 {"messages":[...],"next_before":...}` | Member-only history, newest sequence first; default limit 50, maximum 100 |
| `GET /api/v1/conversations/{id}/messages?after_sequence=42&limit=50` | `200 {"messages":[...],"next_after":...}` | Reconnect sync, oldest missed sequence first; `before_sequence` and `after_sequence` cannot be combined |
| `PATCH /api/v1/conversations/{id}/messages/{message_id}` | `200 {"message":{...}}` | Edit own message; channel moderators with message-management permission may edit others. Body: `{"content":"Updated text"}` |
| `DELETE /api/v1/conversations/{id}/messages/{message_id}` | `200 {"message":{...}}` | Soft-delete own message or moderate a channel message; returns tombstone |
| `PUT /api/v1/conversations/{id}/read-cursor` | `200 {"cursor":{...}}` | Body `{"sequence":42}`; advances caller's cursor monotonically, no further than the latest sequence |
| `GET /api/v1/conversations/{id}/read-cursors` | `200 {"cursors":[...]}` | Member-only; returns visible members' last-read sequence, defaulting to zero |
| `PUT /api/v1/conversations/{id}/delivery-cursor` | `200 {"cursor":{...}}` | Body `{"sequence":42}`; durably acknowledges the caller's highest processed sequence, advances monotonically, and cannot exceed the latest sequence |
| `GET /api/v1/conversations/{id}/delivery-cursors` | `200 {"cursors":[...]}` | Member-only delivery positions, defaulting to zero |
| `GET /api/v1/conversations/{id}/reactions?message_ids=<uuid,...>` | `200 {"reactions":[...]}` | Member-only reaction summaries for up to 100 message IDs; each summary includes emoji, count, and whether the caller reacted |
| `PUT /api/v1/conversations/{id}/messages/{message_id}/reactions` | `200 {"sequence":...,"changed":...,"reactions":[...]}` | Body `{"emoji":"👍"}`; adds the caller's reaction idempotently. Supported built-ins: 👍 ❤️ 😂 🎉 😮 😢 |
| `DELETE /api/v1/conversations/{id}/messages/{message_id}/reactions` | `200 {"sequence":...,"changed":...,"reactions":[...]}` | Body `{"emoji":"👍"}`; removes only the caller's matching reaction; repeated removal is idempotent |

`message` is `{id, conversation_id, sender_id, sequence, client_message_id, reply_to_message_id, mentioned_user_ids, content, created_at, edited_at, deleted_at}`. `reply_to_message_id` is null for a root message and otherwise references a message in the same conversation. `mentioned_user_ids` contains distinct current members matched by case-insensitive `@username` tokens in the text (up to 100 unique names); unknown names and users outside the conversation remain ordinary text. Editing a message recomputes these IDs. Content is plain text, non-blank, at most 4,000 Unicode characters and 16,000 UTF-8 bytes. Sequence numbers are allocated transactionally per conversation. `next_before` is the oldest sequence in the returned page when another page exists; pass it as `before_sequence` to continue. Unknown conversations and non-members both receive `404`.

Each new message, resolved member mentions, and its `message.created` outbox event commit in the same PostgreSQL transaction. Reply and mention IDs appear in history and durable event payloads. Edits recompute mention relationships and append a `message.updated` event; deletes clear mention relationships and append a `message.deleted` event atomically. A process-local publisher polls the outbox and forwards events to connected WebSocket clients. `published_at` means forwarded to this process's bounded event bus, not delivered to every client; reconnect and reload recent history to recover missed events and updates to older messages.

### Implemented: WebSocket message delivery

Connect to `GET /api/v1/realtime` over WebSocket/WSS. The handshake requires the existing session cookie and an exact allowed `Origin`; the session is revalidated every 30 seconds. The server accepts JSON control frames:

```json
{"type":"subscribe","conversation_id":"<uuid>"}
{"type":"unsubscribe","conversation_id":"<uuid>"}
{"type":"typing","conversation_id":"<uuid>","active":true}
{"type":"ack","conversation_id":"<uuid>","sequence":42}
```

Subscriptions are checked against current conversation or channel visibility and capped at 100 per connection. The server sends `subscribed` / `unsubscribed` acknowledgements and `message.created`, `message.updated`, or `message.deleted` events using the envelope described in section 6. Socket writes time out after five seconds; a blocked sink ends the connection so the client can reconnect and sync. PostgreSQL-backed WebSocket integration includes a real slow-reader case that stops consuming frames while maximum-size events create TCP backpressure, then confirms the gateway disconnects it within the bounded timeout. Membership and permissions are rechecked every 30 seconds. The process-local message event bus holds at most 512 messages; a lagging connection is closed with code 1013 and should reconnect, then reload recent history. Heartbeats are sent every 25 seconds; a connection without a pong for 75 seconds is closed. Typing signals are accepted only for subscribed conversations, checked against current membership, throttled per connection, broadcast ephemerally, and acknowledged with `typing_ack`; clients expire the indicator if a stop signal is missed. Reaction state is durable in PostgreSQL; `reaction.added` and `reaction.removed` events are broadcast to other subscribers through a bounded, process-local bus. If a connection falls behind that bus, it closes with code 1013; reconnect reloads reaction summaries for the visible message page. Delivery acknowledgements use the `ack` command with the highest message sequence the client has processed; the server validates membership and sequence, persists a monotonic delivery cursor, and replies with `delivery_ack`. Other subscribers receive `delivery_cursor.updated`; reconnecting clients reload cursors over HTTP. If a client falls behind the read- or delivery-cursor bus, the server closes it with code 1013 so it reconnects and refreshes durable cursor snapshots. PostgreSQL-backed integration tests verify lag-close and reconnect/resubscribe for the message, reaction, read-cursor, delivery-cursor, and community event buses. The gateway is single-instance; events are not shared between server instances. Read-cursor updates are durable in PostgreSQL, broadcast to other subscribed members as `read_cursor.updated`, and reloaded after reconnect. Presence is ephemeral and process-local: first and last WebSocket connections announce online/offline, multiple tabs are counted as one user, and subscribers receive a current status snapshot. Presence is disclosed only to users sharing at least one conversation and only when `presence_visible` is enabled; turning it off sends a clear event.

Community membership and channel mutations emit process-local `members.updated` or `channels.updated` events to connected members of that community. The version 1 envelope carries `payload.community_id`; clients reload the corresponding member, channel, and category resources over HTTP. These events are invalidations rather than authoritative resource snapshots and are not shared across server instances. A connection that falls behind the community event bus is closed with code 1013; on reconnect the client reloads community, channel, category, and open member-management state.

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

Use a versioned event envelope with a unique event ID and enough routing/order metadata for safe processing. `presence.updated` uses `{"type":"presence.updated","version":1,"event_id":"...","occurred_at":"...","payload":{"user_id":"...","online":true}}`; `online` is `false` for offline and `null` to clear a previously visible status when presence is hidden.

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
    "client_message_id": "019...",
    "reply_to_message_id": null,
    "mentioned_user_ids": [],
    "content": "Hello RustChat!"
  }
}
```

The actual payload will depend on encryption mode and the final message schema. Do not assume plaintext content will be present once E2EE is implemented.

## 7. Event naming

Event types are stable, namespaced and versioned (all are version 1 today). Implemented server-to-client events:

| Event | Scope | Payload |
|---|---|---|
| `message.created`, `message.updated`, `message.deleted` | subscribed conversation | message fields; carries `conversation_id` and `sequence` |
| `reaction.added`, `reaction.removed` | subscribed conversation | `message_id`, `user_id`, `emoji`; carries `sequence` |
| `read_cursor.updated` | subscribed conversation, other members | `user_id`, `sequence` |
| `delivery_cursor.updated` | subscribed conversation, other members | `user_id`, `sequence` |
| `typing.started`, `typing.stopped` | subscribed conversation, other members; ephemeral | `user_id` |
| `presence.updated` | users sharing a conversation, subject to `presence_visible` | `user_id`, `online` |
| `channels.updated`, `members.updated` | community members; invalidation only | `community_id` |

Control replies to client commands are not versioned events: `{"type":"subscribed"|"unsubscribed","conversation_id":"..."}`, `typing_ack` (conversation_id), `delivery_ack` (conversation_id, sequence) and `{"type":"error","conversation_id":<id or null>,"code":"..."}`. Error codes are `BAD_REQUEST` (malformed or unknown command), `NOT_FOUND` (no access to the conversation), `SUBSCRIPTION_LIMIT` (more than 100 subscriptions) and `INVALID_DELIVERY_CURSOR`.

Not implemented: `conversation.read`, `channel.created`, `member.joined` and `member.removed`. Read state is delivered as `read_cursor.updated` and community changes as the two invalidation events above.

## 8. Message sending and idempotency

The plain-text send flow is implemented for conversations. The client supplies a stable UUIDv7 `client_message_id`; the database uniqueness scope is `(conversation_id, sender_id, client_message_id)`, and the fingerprint is SHA-256 over the exact content bytes plus the optional reply target.

The client generates a stable UUIDv7 client message ID before sending.

The server:

1. Authenticate the sender.
2. Verify access to the target conversation/channel.
3. Validate the payload and size.
4. Check the idempotency key and request fingerprint.
5. In one transaction, allocate sequence, persist the message and persist its outbox event.
6. Return the canonical message result and sequence.

The same key and same content/reply target resolve to the original message. The same key with different content or reply target returns a conflict. A conversation row lock serializes sequence allocation and retry checks; the message and outbox event commit atomically.

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
