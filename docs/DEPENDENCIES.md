# Dependency and Licence Record

Required by [CLAUDE.md](../CLAUDE.md) section 7. Update this file whenever a dependency is added, removed or upgraded across a major version.

**Status:** reviewed against the current lockfiles and package metadata on 2026-11-16. Metadata is the package author's declared licence expression; this inventory is not a legal review.

**Automated checks.** CI runs `cargo deny check` (policy in `backend/deny.toml`: permissive licences only, crates.io as the only source, RustSec advisories, no wildcard versions), `cargo audit` and `pnpm audit --audit-level moderate` on pushes to `main`, pull requests, and weekly. `cargo audit` scans every package in `Cargo.lock` and reports two ignored advisories (RUSTSEC-2026-0459 for `libsqlite3-sys`, RUSTSEC-2023-0071 for `rsa`). Both crates belong to sqlx's optional SQLite and MySQL drivers, which appear in the lockfile but are not in the build graph for any target or feature set (verified with `cargo tree --all-features --target all -i`), so they do not affect RustChat. They are ignored in `backend/.cargo/audit.toml` with the reasons written down, and a CI step fails if any of those crates ever enters the real build graph. Remove the ignores when sqlx stops listing the drivers. Exact versions are pinned in `backend/Cargo.lock` and `frontend/pnpm-lock.yaml`.

## Backend (Rust)

The Rust lockfile includes direct and transitive dependencies. The current `cargo deny check` policy permits the configured MIT, Apache, BSD (including 0BSD), ISC, Zlib, Unlicense, Unicode, BSL and CDLA-Permissive expressions; it rejects unapproved licences, unknown sources, wildcard versions and advisories not explicitly justified.

Selected direct dependencies:

| Crate | Licence | Used for |
|---|---|---|
| `axum`, `axum-extra` | MIT | HTTP framework, cookie handling |
| `futures-util` | MIT OR Apache-2.0 | Split and drive WebSocket streams |
| `tokio` | MIT | Async runtime |
| `tower`, `http-body-util` (dev) | MIT | Request testing utilities |
| `sqlx` | MIT OR Apache-2.0 | PostgreSQL access and migrations (default features off; no TLS yet) |
| `argon2` | MIT OR Apache-2.0 | Argon2id password hashing (RustCrypto) |
| `sha2` | MIT OR Apache-2.0 | SHA-256 of session tokens (RustCrypto) |
| `getrandom` | MIT OR Apache-2.0 | Operating-system randomness for tokens and salts |
| `base64` | MIT OR Apache-2.0 | Token encoding |
| `uuid` | Apache-2.0 OR MIT | UUIDv7 identifiers |
| `time` | MIT OR Apache-2.0 | Timestamps |
| `serde`, `serde_json` | MIT OR Apache-2.0 | HTTP serialization and JSON outbox payloads |
| `thiserror` | MIT OR Apache-2.0 | Error types |
| `async-trait` | MIT OR Apache-2.0 | Async methods on port traits used as trait objects |
| `tracing`, `tracing-subscriber` | MIT | Structured logging |
| `lettre` | MIT OR Apache-2.0 | Optional SMTP password-reset delivery using Rustls |
| `totp-rs` | MIT | RFC 6238 time-based one-time passwords for two-step verification (default features off; `otpauth` for provisioning URIs) |
| `aes-gcm` | Apache-2.0 OR MIT | AES-256-GCM sealing of stored authenticator secrets (RustCrypto) |
| `tokio-tungstenite` (HTTP dev dependency) | MIT | Real WebSocket API integration tests |

No custom cryptography is implemented; hashing and randomness come from maintained RustCrypto and OS facilities.

## Frontend (npm)

The application depends on React and React DOM. Development dependencies include Vite, TypeScript, Vitest, jsdom, Testing Library, oxlint, Playwright, and axe-core (`@axe-core/playwright`). `pnpm licenses list` also reports MPL-2.0 packages including axe-core and lightningcss. MPL-2.0 is file-level copyleft; check which packages carry it before distributing a modified copy.

## Planned services (not yet used)

Licences in this table are stated from general knowledge and have **not** been verified against the actual images or releases. Verify before adoption.

| Service | Licence | Note |
|---|---|---|
| PostgreSQL 17 and 18 (container images) | PostgreSQL Licence | Supported versions; 18 is the default in the Compose files and 17 is tested in CI |
| Valkey 9 (container image) | BSD-3-Clause | Started by the dev compose file; not used by the backend yet |
| Garage | AGPL-3.0 | Planned for attachments (Phase 2). Run as an unmodified, separate service, AGPL does not extend to RustChat's own code; confirm this and record the decision before adoption |
| LiveKit | Apache-2.0 | Planned for voice (Phase 2) |

## Follow-up

- Re-check the Garage (AGPL-3.0) decision before Phase 2. The project licence is Apache-2.0 (ADR 0007); running Garage as a separate service does not change it.
- Add TLS support to the SQLx connection before any database is reached over an untrusted network.
