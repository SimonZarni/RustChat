# Dependency and Licence Record

Required by [CLAUDE.md](../CLAUDE.md) section 7. Update this file whenever a dependency is added, removed or upgraded across a major version.

**Status:** initial record. Licences were read from package metadata (`cargo metadata`, `pnpm licenses list`) on 2026-11-14. Metadata is the package author's declared licence expression; it is not a legal review.

**Automated checks.** CI runs `cargo deny check` (policy in `backend/deny.toml`: permissive licences only, crates.io as the only source, RustSec advisories, no wildcard versions), `cargo audit` and `pnpm audit --audit-level moderate` on every push and weekly. All three passed locally on 2026-11-14 (the CI job itself has not run yet when this was written). `cargo audit` scans every package in `Cargo.lock` and initially reported two advisories (RUSTSEC-2026-0459 for `libsqlite3-sys`, RUSTSEC-2023-0071 for `rsa`). Both crates belong to sqlx's optional SQLite and MySQL drivers, which appear in the lockfile but are not in the build graph for any target or feature set (verified with `cargo tree --all-features --target all -i`), so they do not affect RustChat. They are ignored in `backend/.cargo/audit.toml` with the reasons written down, and a CI step fails if any of those crates ever enters the real build graph. Remove the ignores when sqlx stops listing the drivers. Exact versions are pinned in `backend/Cargo.lock` and `frontend/pnpm-lock.yaml`.

## Backend (Rust)

231 third-party crates including transitive dependencies. Every crate declares a permissive option (MIT, Apache-2.0, BSD, ISC, Zlib, Unlicense or Unicode-3.0); none is GPL- or AGPL-only. One transitive crate is declared `MIT OR Apache-2.0 OR LGPL-2.1-or-later`, which can be used under its permissive terms.

Direct dependencies:

| Crate | Licence | Used for |
|---|---|---|
| `axum`, `axum-extra` | MIT | HTTP framework, cookie handling |
| `tokio` | MIT | Async runtime |
| `tower`, `http-body-util` (dev) | MIT | Request testing utilities |
| `sqlx` | MIT OR Apache-2.0 | PostgreSQL access and migrations (default features off; no TLS yet) |
| `argon2` | MIT OR Apache-2.0 | Argon2id password hashing (RustCrypto) |
| `sha2` | MIT OR Apache-2.0 | SHA-256 of session tokens (RustCrypto) |
| `getrandom` | MIT OR Apache-2.0 | Operating-system randomness for tokens and salts |
| `base64` | MIT OR Apache-2.0 | Token encoding |
| `uuid` | Apache-2.0 OR MIT | UUIDv7 identifiers |
| `time` | MIT OR Apache-2.0 | Timestamps |
| `serde`, `serde_json` (dev) | MIT OR Apache-2.0 | (De)serialization |
| `thiserror` | MIT OR Apache-2.0 | Error types |
| `async-trait` | MIT OR Apache-2.0 | Async methods on port traits used as trait objects |
| `tracing`, `tracing-subscriber` | MIT | Structured logging |

No custom cryptography is implemented; hashing and randomness come from maintained RustCrypto and OS facilities.

## Frontend (npm)

98 packages reported by `pnpm licenses list`: 83 MIT, 6 Apache-2.0, 2 BSD-2-Clause, 2 BSD-3-Clause, 2 ISC, 2 MPL-2.0, 1 BlueOak-1.0.0. MPL-2.0 is file-level copyleft; it is acceptable for unmodified use as a dependency, but check which packages carry it before distributing a modified copy. Direct dependencies today: React, Vite, TypeScript, Vitest, jsdom, Testing Library, oxlint.

## Planned services (not yet used)

Licences in this table are stated from general knowledge and have **not** been verified against the actual images or releases. Verify before adoption.

| Service | Licence | Note |
|---|---|---|
| PostgreSQL 18 (container image) | PostgreSQL Licence | In use for development |
| Valkey 9 (container image) | BSD-3-Clause | Started by the dev compose file; not used by the backend yet |
| Garage | AGPL-3.0 | Planned for attachments (Phase 2). Run as an unmodified, separate service, AGPL does not extend to RustChat's own code; confirm this and record the decision before adoption |
| LiveKit | Apache-2.0 | Planned for voice (Phase 2) |

## Follow-up

- Re-check the Garage (AGPL-3.0) decision before Phase 2. The project licence is Apache-2.0 (ADR 0007); running Garage as a separate service does not change it.
- Add TLS support to the SQLx connection before any database is reached over an untrusted network.
