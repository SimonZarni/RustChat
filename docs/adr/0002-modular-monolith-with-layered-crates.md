# 0002. Modular monolith with layered crates

- **Status:** Accepted
- **Date:** 2026-11-14

## Context

RustChat must be easy to develop and to self-host, while keeping business rules testable and independent of Axum, SQLx, Valkey and LiveKit ([ARCHITECTURE.md](../ARCHITECTURE.md), `CLAUDE.md` section 2). Scaling needs are unmeasured.

## Decision

Build one deployable server as a Cargo workspace with layered crates:

- `domain`: entities and rules, no framework or infrastructure dependencies;
- `application`: use cases and the ports (traits) they need;
- `infrastructure`: implementations of those ports (PostgreSQL, Argon2, clock, rate limiter);
- `http`: the HTTP adapter;
- `apps/server`: the composition root that wires concrete types.

Dependencies point inward only. Application services hold ports as trait objects. Crates and domain areas are added when there is code for them, not as empty placeholders.

## Alternatives considered

- **Microservices from the start:** rejected as unnecessary operational complexity without measurements to justify it.
- **A single crate with modules:** simpler, but the dependency rule would be a convention rather than something the compiler enforces.
- **Generics instead of trait objects for ports:** avoids dynamic dispatch, but spreads type parameters through every layer. Dynamic dispatch cost is negligible next to database and hashing work.

## Consequences

Use cases run in unit tests against in-memory fakes, and infrastructure is tested against real PostgreSQL. Adding a capability costs a port plus an implementation. The `async-trait` crate is used because native async traits are not dyn-compatible. The layout can later be split into separately scaled processes along the same seams if measurements justify it. See the "Current state" note in `ARCHITECTURE.md` for the crates that exist today.
