# 0011. PostgreSQL-shared authentication rate limits

- **Status:** Accepted
- **Date:** 2026-11-16
- **Supersedes:** [ADR 0006](0006-in-process-rate-limiting-behind-a-port.md) for the production rate-limit implementation

## Context

The initial in-memory rate limiter loses counters on restart and gives each server process a separate budget. Authentication limits should persist across restarts and be shared by servers that use the same PostgreSQL database without adding Valkey as an application dependency.

## Decision

- Keep the application `RateLimiter` port and use PostgreSQL as the production adapter.
- Store only SHA-256 digests of limiter keys, with counts and expiry timestamps; remove expired rows during writes.
- Keep the in-memory adapter for unit and API test fixtures.
- Configure the login-per-IP, failed-login-per-username and registration-per-IP thresholds through environment variables. Keep their current time windows fixed.
- Fail authentication requests closed when rate-limit storage is unavailable.

## Alternatives considered

- **Continue with process-local counters:** simpler, but counters reset on restart and do not combine across server processes.
- **Use Valkey:** supports shared counters but introduces an otherwise unused runtime service and another availability dependency.

## Consequences

Instances sharing the same PostgreSQL database share authentication budgets, and counters survive process restarts. Rate-limit records contain no raw username or IP address. PostgreSQL availability is required for authentication rate-limit checks. Realtime WebSocket delivery and presence still use process-local buses and are not made multi-instance by this decision.
