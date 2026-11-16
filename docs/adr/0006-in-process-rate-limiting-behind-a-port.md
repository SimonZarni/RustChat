# 0006. In-process rate limiting behind a port

- **Status:** Superseded by 0011
- **Date:** 2026-11-14

## Context

Authentication endpoints need brute-force and abuse protection before the server is exposed ([THREAT_MODEL_AUTH.md](../THREAT_MODEL_AUTH.md)). Deployment today is a single instance. Valkey is available in the dev stack but not yet used by the backend.

## Decision

Define a `RateLimiter` port in the application layer and implement it in process: fixed-window counters in memory, capped at 100,000 keys with eviction so memory is bounded. Policy lives in the application layer: login attempts per client network (any outcome), login failures per username (unknown usernames counted too), and registration attempts per client network. Limits run before password hashing. The client network is the TCP peer address unless the operator declares trusted proxy hops, then the matching `X-Forwarded-For` entry.

## Alternatives considered

- **Valkey-backed counters now:** correct across instances, but adds a runtime dependency and a failure mode (fail open or closed) for a deployment that has one instance.
- **Rely on the reverse proxy alone:** useful as defence in depth, but a proxy cannot count failed logins per username.
- **Account lockout until an administrator unlocks:** easy to abuse for denial of service and needs admin tooling.

## Consequences

This was the initial implementation and remains useful as an in-memory test adapter. Its counters reset on restart and are not shared across server processes. Production now uses the PostgreSQL implementation described in [ADR 0011](0011-postgresql-shared-auth-rate-limits.md). The account lockout trade-off remains: an attacker can keep a victim's logins blocked for one window by failing on purpose; existing sessions are unaffected.
