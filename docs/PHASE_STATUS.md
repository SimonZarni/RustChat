# Phase Completion Audit

Last reviewed: 2026-11-15. The PRD remains the source of product scope; this file records implementation status against its Phase 0 exit criteria, Phase 1 MVP definition of done, and P0 requirements.

## Phase 0 — complete

| Exit criterion | Status | Evidence |
|---|---|---|
| Approved product and architecture decisions | Complete | `docs/RustChat-PRD.md`, `docs/ARCHITECTURE.md`, accepted records in `docs/adr/` |
| Repository and workspace setup | Complete | Rust workspace in `backend/`; React/TypeScript workspace in `frontend/` |
| Local development environment and documented build, test, run steps | Complete | `README.md`; backend and frontend checks ran locally during this review |
| CI quality gates | Complete | `.github/workflows/ci.yml` runs Rust formatting, Clippy and tests; frontend typecheck, lint, Vitest, Chromium Playwright accessibility/end-to-end tests and build; dependency checks |
| Initial migration strategy | Complete | `docs/MIGRATIONS.md`; SQLx migrations 0001–0020; fresh PostgreSQL migrations exercised by integration tests |
| Authentication and authorization threat model | Complete as an initial baseline | `docs/THREAT_MODEL_AUTH.md`, updated alongside authentication and authorization implementation |

Phase 0's exit criterion is met: a contributor can follow the documented local setup and build, test, and run the minimal application. This does not imply the later MVP or production release criteria are met.

## Phase 1 — in progress

### Implemented

- Username/password registration and login, logout, revocable sessions, web workflows for changing passwords and viewing/revoking sessions, authenticated password change (retaining the current session and revoking other sessions atomically), web account deactivation and confirmed deletion, session revocation, login reactivation, profile scrubbing, retained message history, Argon2id password hashing, registration policy, and authentication rate limits. Community owners must transfer ownership before deactivation or deletion.
- Optional email-based password recovery with SMTP delivery, generic request responses, one-hour single-use tokens stored as SHA-256 digests, atomic reset/session revocation, and a browser flow. Real-PostgreSQL integration covers unknown addresses, expiry, replay rejection, successful reset, and prior-session revocation. Email verification is deferred by product decision.
- Editable usernames and basic profiles with display name and bio.
- User configured profile discoverability and visibility, with an exact username profile preview and matching server side restrictions on new conversation creation.
- User-configurable presence visibility, process-local online/offline transitions counted across multiple tabs, conversation-scoped status snapshots, and client status indicators on direct conversations.
- Direct and group conversations, membership checks, and group-size limits.
- Durable, ordered, cursor-paginated messages with idempotent sends, same-conversation replies, member-scoped mentions, edits, soft deletes, emoji reactions, a transactional outbox, and durable read and delivery cursors.
- Per-conversation local unsent drafts stored in the browser and restored when returning to a conversation.
- Communities, invitations, ordered categories, categorized text channels, roles, permission overrides, membership administration, ownership transfer, community deletion, and audit records in the backend; the web client exposes core messaging and community-management workflows, including invitation listing and revocation.
- `Ctrl+K` / `⌘K` opens the new-conversation dialog, `Escape` closes open dialogs, and the shortcut hint follows the platform modifier.
- Light and dark theme preference with browser persistence and matching controls across the main interface.
- Messages render basic Markdown emphasis, HTTP(S) links, lists, inline code, and fenced code blocks as safe React text; message and connection updates expose polite live-region semantics, reduced-motion preferences are honored, and modal focus is contained and restored.
- Authenticated single-instance WebSocket message delivery with reply and mention references, privacy-scoped presence, typing, reaction and delivery-ack events, read-cursor updates, community-scoped member/channel invalidation events, bounded queues, heartbeats, permission rechecks, and reconnect recovery through durable message history and cursor/reaction summaries. Read/delivery-cursor bus lag closes with code 1013; the client refetches persisted cursor snapshots after reconnect.
- Initial responsive web client and Docker Compose/Caddy/PostgreSQL deployment baseline with separate runtime/migrator roles, database-backed readiness checks, shared PostgreSQL authentication rate limits, private-network HTTP response metrics, and backup/restore scripts. A clean-volume Compose rehearsal built both images, applied the then-current migrations, ran authenticated account and message workflows, created/restored a backup, then verified health, the restored session, and message history.

### Remaining against the PRD P0 requirements and MVP definition of done

- **Account and profile workflows:** email verification is deferred by current product decision and is not part of the current release scope.
- **Conversation workflows:** drafts are local to the browser and device; synchronized drafts are not provided. Synchronized drafts are not a Phase 1 MVP exit criterion.
- **Realtime coverage:** member/channel changes emit process-local community-scoped invalidation events; connected authorized clients reload member/channel state. WebSocket integration covers member/channel event delivery to community members and non-disclosure to outsiders, plus presence privacy, shared-conversation scoping, multiple tabs, delivery acknowledgements, persistence and invalid sequence rejection. A frontend reconnect test verifies durable read-cursor refresh; PostgreSQL-backed server integration verifies all durable bus types (message, read cursor, reaction, delivery cursor, community) close lagged sockets with code 1013 and clients can reconnect/resubscribe. A real WebSocket slow-reader test now holds reads while sending maximum-size events to exercise TCP backpressure against the bounded send timeout. Presence and realtime events use process-local buses; presence resets with server restart. The gateway is single-instance.
- **Community/channel/role completeness:** server-side role hierarchy excludes the default role from rank calculations, prevents delegated managers from changing equal/higher roles or moving a subordinate above themselves, and limits delegated permissions. API integration covers these denials and mutually conflicting channel allow/deny bits.
- **User experience:** light/dark themes, modal focus handling, reduced-motion support, live status announcements, and basic message formatting are implemented. Playwright with axe-core checks WCAG 2.1 A/AA for the authenticated workspace, new-conversation dialog, populated message history and created-channel workspace in light mode, plus the workspace in dark mode. Browser review verifies named landmarks, modal names, keyboard navigation through core controls and administration, focus trapping/restoration, the labelled polite message log and mobile-width overflow. Contrast issues found in populated conversation/channel states were corrected. A manual spoken-output review with a screen reader remains outstanding; axe and browser accessibility-tree checks cannot validate the actual assistive-technology experience.
- **Self-hosting operations:** database-backed readiness and release-tagged upgrade/rollback guidance are implemented; the fresh-volume Compose and restore rehearsal passed locally. Shared PostgreSQL authentication rate limiting is implemented and tested. Instance administrators are configured by user ID and can list the newest 100 accounts, suspend/restore non-admin users, revoke sessions, and review audit records. DB pool size, group size, and auth rate thresholds are configurable, while other operational limits remain fixed. Metrics currently cover low-cardinality HTTP response totals only; the runbook has not been rehearsed on a production host.

The PRD's core communication exit criterion is implemented and covered by PostgreSQL-backed API/WebSocket tests and browser workflow tests. Phase 1 remains **in progress** until the required screen-reader experience receives a manual assistive-technology review. Email verification remains deferred by product decision. The local deployment/restore workflow has been exercised; a production host and disaster-recovery objective remain unvalidated and are operational follow-up, not a Phase 1 communication feature gap.

## Checks run during this review

- `cargo fmt --all -- --check` — passed.
- `cargo clippy --workspace --all-targets -- -D warnings` — passed. `cargo test -p rustchat-http --lib realtime::tests::stalled_socket_send_times_out` — passed. `cargo test -p rustchat-http --test auth_api websocket_closes_and_allows_reconnect_after_read_cursor_bus_lag` — passed against PostgreSQL 18. `cargo test -p rustchat-server config::tests` — passed (9 config tests).
- `cargo test --workspace` against a fresh PostgreSQL 18 database — passed (135 Rust tests across unit, API, rate-limit, WebSocket, and PostgreSQL integration suites; includes instance-admin suspension/audit/session revocation and a real slow-reader socket, along with recovery expiry/replay, community authorization, shared rate-limit state, and lag recovery for durable event buses).
- `pnpm typecheck` — passed.
- `pnpm lint` — passed with three `react(set-state-in-effect)` warnings and one hook dependency warning in `frontend/apps/web/src/App.tsx`.
- `pnpm test` — passed (8 frontend unit/component tests covering password recovery, invite management, keyboard shortcut/focus handling, formatting, reconnect, theme, account/profile and message workflows; includes jsdom axe scans).
- `pnpm test:e2e` — passed (5 Chromium tests: axe WCAG 2.1 A/AA scans of sign-in/workspace/dialog/message/channel states, workspace/modal keyboard focus and restoration, workspace navigation through community/settings/admin controls, conversation/message/community/channel browser workflow, narrow viewport overflow, and dark-theme contrast).
- `pnpm build` — passed with a 512 kB JavaScript chunk warning (500 kB advisory threshold).
- `cargo audit` — passed; no RustSec vulnerabilities reported.
- `cargo deny check` — passed with dependency hygiene warnings, including duplicate transitive versions and an unused license allowance.
- `git diff --check` — passed.
