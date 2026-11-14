# 0007. Apache-2.0 licence

- **Status:** Accepted
- **Date:** 2026-11-14

## Context

The repository is public with no licence, which legally means all rights are reserved: others may read the code but not use it. The project wants contributors and self-hosters. All current dependencies carry a permissive option, so no licence is forced upon us ([DEPENDENCIES.md](../DEPENDENCIES.md)).

## Decision

License RustChat under the Apache License 2.0. The licence text is in the root `LICENSE` file, and `Cargo.toml` and `package.json` files declare `Apache-2.0`. Contributions are accepted under the same licence.

## Alternatives considered

- **MIT:** shorter, but has no explicit patent grant.
- **MIT OR Apache-2.0:** common in the Rust ecosystem, with maximum compatibility, but unnecessary complexity for an application rather than a library.
- **AGPL-3.0:** would require operators of modified hosted forks to publish changes, but discourages some adopters and contributors.

## Consequences

Anyone may use, modify, host and sell RustChat, including in closed-source products; there is no obligation to share changes. Hosted forks by third parties are therefore possible. The decision is hard to reverse for code already distributed under it. Dependencies licensed under copyleft terms (Garage, AGPL-3.0) are used only as separate services and must be kept that way. A `NOTICE` file and per-file headers are not required and are not added.
