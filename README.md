# RustChat

RustChat is a self-hostable, real-time communication platform designed around a secure Rust backend and a shared React frontend for web and desktop.

The project is intended to provide direct messaging, group conversations, communities, channels, file attachments and voice communication. The initial architecture is a modular monolith: straightforward to develop and deploy, with clear internal boundaries that allow individual components to scale independently later if measurements justify it.

## Project status

**Status:** Phase 0 exit criteria are met: documented local setup, workspace build/test/run path, CI quality gates, migration strategy, and an initial threat model. Phase 1 has working account/session APIs, password change with other-session revocation, web workflows for account deactivation and retained-history deletion, and editable display-name/bio profiles; direct/group conversations; community/channel/role permission APIs with client controls for settings, channels, roles, member removal, invitation listing and revocation, ownership and deletion; durable editable/deletable messages with paginated history, idempotent sends, replies, member mentions, emoji reactions, basic Markdown, and per-conversation local drafts; a transactional outbox; WebSocket message delivery that works across several server instances sharing one database, privacy-scoped presence, typing and reaction events, community-scoped member/channel invalidations, durable read and delivery cursors, and reconnect recovery; an initial web client with sent/delivered/read markers, light/dark themes, keyboard shortcuts, modal focus handling and reduced-motion support; and a Docker Compose deployment with backup/restore scripts and a release-tagged upgrade/rollback runbook. Instance administrators can list and suspend accounts through the admin API and web controls. Backend integration tests exercise APIs and realtime behavior against PostgreSQL, including a real slow-reader socket. Playwright covers keyboard navigation and core conversation/message/community/channel workflows using stateful mocked API responses. Phase 1 remains **in progress** pending a manual screen-reader review; recovery email addresses are verified by an emailed link. Password reset is implemented with optional SMTP delivery, single-use hashed tokens and session revocation. The Docker Compose baseline and backup/restore flow passed a fresh-volume local rehearsal that verified restored sign-in and message history. See [API and realtime protocol](docs/API_AND_REALTIME.md) for exact implemented routes and limitations.

See the [Phase Completion Audit](docs/PHASE_STATUS.md) for the reviewed Phase 0/1 feature inventory, outstanding P0 requirements and checks run.

Phase 2 has started: attachments include optional S3-compatible storage, scoped signed upload/download URLs, scheduled expired-upload cleanup, and paired database/object-store snapshot scripts. Friends and blocking include request management, relationship controls, and server-side privacy enforcement. The initial in-app notifications inbox covers direct messages, mentions, and friend requests with unread counts and realtime refresh. Storage still requires an operator-provisioned bucket; malware scanning, inline previews, browser/desktop notifications, and notification preferences remain planned. See [deployment configuration](docs/DEPLOYMENT.md).

The technology stack describes the intended direction. It does not imply that every listed feature or integration has already been implemented. Update this section as working functionality becomes available.

## Core principles

- Self-hostable without mandatory paid SaaS services.
- Open-source dependencies with licences reviewed before adoption.
- Security and authorization enforced by the backend.
- Clear domain and application boundaries.
- Reliable message persistence, retries and synchronization.
- Bounded resource usage and measurable performance.
- Automated tests for backend, frontend and end-to-end workflows.
- No custom cryptographic algorithms.

## Technology stack

The lists below are the intended stack. The current frontend uses React, TypeScript, Vite, plain CSS, and React state with `fetch`, and does not yet use Tailwind CSS, shadcn/ui, TanStack Query, Zustand, Zod or MSW. Valkey, LiveKit and Tauri are not implemented; Garage is selected but not bundled or provisioned by Compose. See [ARCHITECTURE.md](docs/ARCHITECTURE.md).

### Backend
- Rust, Tokio and Axum
- SQLx
- PostgreSQL
- Valkey
- WebSockets
- S3-compatible object storage via AWS Rust SDK (Garage selected)
- LiveKit for voice/media

### Frontend
- React and TypeScript
- Vite
- Tailwind CSS and shadcn/ui
- TanStack Query for server state
- Zustand for local UI state
- Zod for validation
- Tauri 2 for the desktop application

### Quality and operations
- Vitest, React Testing Library and MSW
- Playwright for browser end-to-end tests
- Rust unit and integration tests
- k6 for load testing
- Docker Engine and Docker Compose
- Caddy, Prometheus and Grafana

All dependencies must be reviewed for current maintenance, compatibility, security and licensing before being added. Free software does not make hosting, domains, bandwidth or storage free.

## Planned capabilities

1. User accounts, authentication and device/session management.
2. Direct and group conversations.
3. Communities, channels, memberships, roles and permissions.
4. Realtime messaging, presence, typing indicators and read state.
5. File attachments.
6. Voice communication through a self-hosted media server.
7. End-to-end encryption introduced as a separately designed and reviewed milestone.
8. Web client and desktop application sharing the same frontend codebase.
9. Self-hosted deployment and administration.

## Repository layout

The repository's current structure is summarized below; backend and frontend tests live with their crates and applications. Add modules when there is a real use case rather than creating empty folders for planned features.

```text
rustchat/
├── README.md
├── LICENSE
├── CLAUDE.md
├── docs/
│   ├── RustChat-PRD.md
│   ├── UI-DESIGN-SPECIFICATION.md
│   ├── ARCHITECTURE.md
│   ├── DATABASE.md
│   ├── API_AND_REALTIME.md
│   ├── SECURITY.md
│   ├── THREAT_MODEL_AUTH.md
│   ├── MIGRATIONS.md
│   ├── DEPENDENCIES.md
│   ├── CONTRIBUTING.md
│   └── adr/
├── backend/
├── frontend/
├── deployment/
```

## Getting started

### Prerequisites

- Rust (stable) via [rustup](https://rustup.rs)
- Node.js 22+ and pnpm 10 (`corepack enable pnpm`; the version is pinned in `frontend/package.json`)
- Docker with Compose, for local PostgreSQL and Valkey

### Backend

The backend tests include integration tests against a real PostgreSQL, so start the [local services](#local-services) first and point `DATABASE_URL` at them. The database role needs permission to create databases (the dev compose role does).

```bash
cd backend
export DATABASE_URL=postgres://rustchat:<password from deployment/.env>@127.0.0.1:5432/rustchat
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# Run the server (apply migrations first; the server never migrates on startup)
export RUSTCHAT_ALLOWED_ORIGINS=http://localhost:5173
export RUSTCHAT_COOKIE_SECURE=false      # plain-HTTP local development only
cargo run -p rustchat-server -- migrate
cargo run -p rustchat-server             # serves on 127.0.0.1:8080
```

| Variable | Required | Default | Meaning |
|---|---|---|---|
| `DATABASE_URL` | yes | | PostgreSQL connection URL (contains a password; never logged) |
| `RUSTCHAT_ALLOWED_ORIGINS` | yes | | Comma-separated exact origins allowed to make state-changing requests, e.g. `https://chat.example.com` |
| `RUSTCHAT_BIND` | no | `127.0.0.1:8080` | Listen address |
| `RUSTCHAT_REGISTRATION` | no | `open` | `open` or `closed` |
| `RUSTCHAT_COOKIE_SECURE` | no | `true` | Set `false` only for local HTTP development |
| `RUSTCHAT_DB_MAX_CONNECTIONS` | no | `10` | Connection pool size |
| `RUSTCHAT_TRUSTED_PROXY_HOPS` | no | `0` | Number of reverse proxies in front of the server. `0` uses the TCP peer address for rate limiting. Set `1` behind a single proxy such as Caddy, and only if clients cannot reach the server directly. The Compose deployment sets it to `1` |
| `RUSTCHAT_GROUP_MEMBER_LIMIT` | no | `100` | Maximum members in a group conversation, including its creator (allowed range: 2–500) |
| `RUSTCHAT_MFA_ENCRYPTION_KEY` | no | unset | 64 hexadecimal characters (32 random bytes, for example `openssl rand -hex 32`) that protect stored authenticator secrets. Setting it enables two-step verification; leave it unset to keep that off. Keep it secret and back it up separately from the database: without it, enrolled authenticators cannot be verified and users must sign in with recovery codes |
| `RUSTCHAT_MFA_ISSUER` | no | `RustChat` | Service name shown in authenticator apps (up to 64 characters, no colon) |
| `RUSTCHAT_SESSION_IDLE_TIMEOUT_DAYS` | no | `7` | Sessions end after this many days without activity (1–30); the absolute lifetime stays 30 days |
| `RUSTCHAT_ADMIN_USER_IDS` | no | empty | Comma-separated UUIDs of instance administrators; community roles do not grant this authority |
| `RUSTCHAT_LOGIN_PER_IP_LIMIT` | no | `30` | Login attempts of any outcome per network per 15 minutes |
| `RUSTCHAT_LOGIN_PER_USERNAME_LIMIT` | no | `10` | Failed login attempts per username per 15 minutes |
| `RUSTCHAT_REGISTER_PER_IP_LIMIT` | no | `5` | Registration attempts per network per hour |
| `RUSTCHAT_API_READ_LIMIT` | no | `600` | Authenticated read requests per user per minute (1–100000; per server process) |
| `RUSTCHAT_API_WRITE_LIMIT` | no | `120` | Other authenticated write requests per user per minute |
| `RUSTCHAT_MESSAGE_SEND_LIMIT` | no | `60` | Messages sent per user per minute |
| `RUSTCHAT_CREATE_LIMIT` | no | `30` | Conversations, communities, channels, categories, roles and invites created per user per hour |
| `RUSTCHAT_UNAUTHENTICATED_LIMIT` | no | `120` | Requests that fail authentication per client network per minute |
| `RUSTCHAT_SMTP_HOST`, `RUSTCHAT_EMAIL_FROM`, `RUSTCHAT_PUBLIC_URL` | no | unset | Configure together to enable password recovery email; see [deployment configuration](docs/DEPLOYMENT.md) |
| `RUSTCHAT_SMTP_PORT` | no | `465` | SMTP port when email delivery is configured |
| `RUSTCHAT_SMTP_USERNAME`, `RUSTCHAT_SMTP_PASSWORD` | no | unset | Optional SMTP credentials; configure both or neither |

### Frontend

```bash
cd frontend
pnpm install
pnpm typecheck && pnpm lint && pnpm test && pnpm build
pnpm dev
```

Run the Chromium keyboard and accessibility checks with `pnpm --filter @rustchat/web exec playwright install chromium` followed by `pnpm --filter @rustchat/web test:e2e`.

The Vite development server proxies `/api` and WebSocket requests to `http://127.0.0.1:8080`. Configure the backend with `RUSTCHAT_ALLOWED_ORIGINS=http://localhost:5173` and `RUSTCHAT_COOKIE_SECURE=false` for local HTTP development.

### Local services

```bash
cp deployment/.env.example deployment/.env    # then change the password
docker compose --env-file deployment/.env -f deployment/compose.dev.yml up -d --wait
```

This starts PostgreSQL and Valkey bound to `127.0.0.1` only. The backend uses PostgreSQL; it does not use Valkey yet.

#### Alternative: native PostgreSQL on Windows (no Docker)

Docker is only one way to get a PostgreSQL (versions 17 and 18 are supported; the examples use 18). The backend needs nothing from Docker itself, so a native install works equally well (verified on Windows 11 with the full backend test suite):

```powershell
# Elevated PowerShell. Choose your own superuser password; keep it out of the repository.
winget install --id PostgreSQL.PostgreSQL.18 --exact --silent --override "--mode unattended --unattendedmodeui none --superpassword <superuser-password> --serverport 5432 --disable-components pgAdmin,stackbuilder"
```

Then, as the `postgres` superuser, restrict it to loopback and create the application role and database (the role needs `CREATEDB` so each test can create its own database, but not superuser):

```sql
ALTER SYSTEM SET listen_addresses = 'localhost';   -- then restart the postgresql-x64-18 service
CREATE ROLE rustchat LOGIN CREATEDB PASSWORD '<password>';
CREATE DATABASE rustchat OWNER rustchat;
```

Use that role's password in `DATABASE_URL` as shown in the Backend section. Only one PostgreSQL can listen on port 5432: stop the Docker one (`docker compose ... stop postgres`) or pick another port in the URL. The service starts automatically with Windows.

Do not copy commands into this guide until they have been tested against the repository.

## Documentation

- [Product requirements (PRD)](docs/RustChat-PRD.md)
- [UI design specification](docs/UI-DESIGN-SPECIFICATION.md)
- [Manual screen-reader review](docs/ACCESSIBILITY_REVIEW.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Database](docs/DATABASE.md)
- [API and realtime protocol](docs/API_AND_REALTIME.md)
- [Security](docs/SECURITY.md)
- [Authentication threat model](docs/THREAT_MODEL_AUTH.md)
- [Migration strategy](docs/MIGRATIONS.md)
- [Dependency and licence record](docs/DEPENDENCIES.md)
- [Architecture decision records](docs/adr/README.md)
- [Security policy and vulnerability reporting](.github/SECURITY.md)
- [Agent instructions](CLAUDE.md)
- [Contributing](docs/CONTRIBUTING.md)

## Licence

RustChat is licensed under the [Apache License 2.0](LICENSE) (see [ADR 0007](docs/adr/0007-apache-2-licence.md)). Contributions are accepted under the same licence. Dependencies keep their own licences; see the [dependency and licence record](docs/DEPENDENCIES.md).
