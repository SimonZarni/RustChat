# 0009. Single-instance WebSocket delivery from the PostgreSQL outbox

- **Status:** Partly superseded by [0014](0014-multi-instance-realtime-through-postgresql.md) (the single-instance constraint)
- **Date:** 2026-11-14

## Context

Messages are persisted with a transactional outbox, but clients need low-latency updates and a way to recover missed events. RustChat currently runs as one server instance; multi-instance deployment and a shared event broker are not implemented. WebSocket connections use the existing server-side session cookie and need conversation-level authorization.

## Decision

- Run an in-process outbox poller that forwards committed outbox records (`message.created`, `message.updated`, `message.deleted`) to a bounded Tokio broadcast channel; reaction, cursor, presence, typing and community events later gained their own bounded process-local buses.
- Authenticate WebSocket handshakes with the existing session cookie and exact configured `Origin` allow-list. Never put session tokens in URLs.
- Require membership when a connection subscribes, cap each connection at 100 subscriptions, and periodically revalidate the session and membership.
- Close lagged connections rather than buffering without a bound. Clients reconnect and recover from the durable history API using `after_sequence`.
- Treat PostgreSQL message history and sequence numbers as authoritative. Process-local WebSocket delivery is an optimization and is not guaranteed to reach every connected client.

## Alternatives considered

- **Valkey Pub/Sub now:** would add a runtime dependency and cross-instance coordination before multiple instances are supported.
- **WebSocket delivery without an outbox:** risks losing committed events when the process stops between database commit and publish.
- **Durable broker now:** adds operational complexity before measured scale requires it. The outbox keeps a later broker behind the existing boundary.

## Consequences

Single-instance delivery is straightforward and bounded. Polling adds a small amount of database work. A crash or slow client can miss an event, so clients must deduplicate and sync by sequence after reconnect. `published_at` records forwarding to the local event bus, not delivery to all clients. Multi-instance deployment requires shared event fan-out and a claim/lease strategy for outbox rows; this implementation does not provide it.
