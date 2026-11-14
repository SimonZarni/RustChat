# 0006. In-process rate limiting behind a port

- **Status:** Accepted
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

No new dependency, easy to test, fast. State is lost on restart and is not shared, so running several instances multiplies the effective limits: a shared implementation of the same port is required before scaling out. An attacker can keep a victim's logins blocked for one window by failing on purpose; existing sessions are unaffected. Limit values are defaults and not yet configurable. Revisit when a second instance is planned.
