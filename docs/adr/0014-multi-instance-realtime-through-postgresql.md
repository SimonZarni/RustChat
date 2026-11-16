# 0014. Multi-instance realtime through PostgreSQL

- **Status:** Accepted
- **Date:** 2026-11-16
- **Supersedes:** the single-instance constraint of [ADR 0009](0009-single-instance-websocket-outbox-dispatch.md)

## Context

ADR 0009 delivered realtime events from process-local buses, which only works with one server process: a second instance would dispatch the same outbox rows again (duplicates), and clients connected to a different instance would never hear events published on the first. Typing, reactions, cursors, community changes and presence had the same limit. Operators asked for the option of running more than one instance (rolling upgrades, more capacity) without adding a new service.

## Decision

Keep PostgreSQL as the only coordination service and avoid Valkey or a broker.

- **Event sharing.** Each instance still delivers an event to its own WebSocket clients from local broadcast buses, immediately. It also hands the event to an `EventBus` port. The PostgreSQL adapter publishes it with `NOTIFY` on one channel through a bounded queue (1,024 events, newest dropped when full), and every instance `LISTEN`s, ignoring notifications it published itself and replaying the others into its local buses. Events are small JSON documents; anything over 7,500 bytes is dropped, because a notification is limited to 8,000 bytes.
- **Message events carry only an id.** A message can be up to 16,000 bytes, so instances announce the message id and receivers read the current message from the database before delivering it.
- **Events are hints, not records.** Messages, history and cursors remain durable in PostgreSQL. If an instance's listening connection is lost, notifications sent meanwhile are gone, so the instance signals every WebSocket to close with code 1013; clients already reconnect and resynchronize from history.
- **Outbox dispatch with leases.** Rows are claimed with `UPDATE ... WHERE id IN (SELECT ... FOR UPDATE SKIP LOCKED) ... claimed_until = now() + 30 s`, so instances dispatch disjoint events. A row is marked published only after delivery, so a crash leaves it to be re-claimed after the lease expires: delivery stays at least once, and clients already deduplicate.
- **Shared presence.** Each instance records its connected users in `presence_connections` and refreshes them every 20 seconds. A user is online while any instance has a row refreshed within 60 seconds. The earliest-connected live row decides the online transition, so concurrent connects announce online once; offline is announced when the last live row goes away. A crashed instance's rows expire without an offline event.
- **Authentication limits** were already shared through PostgreSQL (ADR 0011). The general API limits (see [API_AND_REALTIME.md](../API_AND_REALTIME.md)) stay per process, so each instance has its own budget.

## Alternatives considered

- **Valkey Pub/Sub or NATS:** purpose-built, but adds a service to deploy, secure and monitor for a workload PostgreSQL handles at this scale.
- **Sticky routing to one instance per conversation:** avoids fan-out, but complicates the proxy and still needs shared presence and failover.
- **Send full events through NOTIFY:** simpler, but cannot carry large messages.

## Consequences

- Operators can run several `server` replicas behind Caddy against one PostgreSQL. Each instance holds one extra database connection for listening and one for its notify publisher's occasional use.
- Notifications add load proportional to event rate on every instance; at larger scale a dedicated broker may be warranted (revisit then).
- Presence for a user whose instance crashes can stay "online" for up to a minute and receives no offline event.
- `NOTIFY` is delivered only to sessions listening at that moment; the 1013 resynchronization is the safety net, and it is exercised by tests.
- Rate limits for general API traffic are per instance, so the effective limit scales with the number of instances.
