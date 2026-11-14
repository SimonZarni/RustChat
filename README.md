# RustChat

RustChat is a self-hostable, real-time communication platform designed around a secure Rust backend and a shared React frontend for web and desktop.

The project is intended to provide direct messaging, group conversations, communities, channels, file attachments and voice communication. The initial architecture is a modular monolith: straightforward to develop and deploy, with clear internal boundaries that allow individual components to scale independently later if measurements justify it.

## Project status

**Status:** Phase 0 (foundation). The repository has a minimal Rust server (health endpoint only), a minimal React web shell, CI definitions and a local development compose file. No product features are implemented yet.

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

## Planned technology stack

### Backend
- Rust, Tokio and Axum
- SQLx
- PostgreSQL
- Valkey
- WebSockets
- Garage (S3-compatible) for object storage
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

The exact layout may evolve as implementation begins. Prefer adding modules when there is a real use case rather than creating empty folders for every planned feature.

```text
rustchat/
├── README.md
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
│   └── CONTRIBUTING.md
├── backend/
├── frontend/
├── deployment/
└── tests/
```

## Getting started

### Prerequisites

- Rust (stable) via [rustup](https://rustup.rs)
- Node.js 22+ and pnpm 10 (`corepack enable pnpm`; the version is pinned in `frontend/package.json`)
- Docker with Compose, for local PostgreSQL and Valkey

### Backend

```bash
cd backend
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p rustchat-server        # listens on 127.0.0.1:8080; override with RUSTCHAT_BIND
```

### Frontend

```bash
cd frontend
pnpm install
pnpm typecheck && pnpm lint && pnpm test && pnpm build
pnpm dev
```

### Local services

```bash
cp deployment/.env.example deployment/.env    # then change the password
docker compose --env-file deployment/.env -f deployment/compose.dev.yml up -d --wait
```

This starts PostgreSQL and Valkey bound to `127.0.0.1` only. The backend does not use them yet.

Do not copy commands into this guide until they have been tested against the repository.

## Documentation

- [Product requirements (PRD)](docs/RustChat-PRD.md)
- [UI design specification](docs/UI-DESIGN-SPECIFICATION.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Database](docs/DATABASE.md)
- [API and realtime protocol](docs/API_AND_REALTIME.md)
- [Security](docs/SECURITY.md)
- [Authentication threat model](docs/THREAT_MODEL_AUTH.md)
- [Migration strategy](docs/MIGRATIONS.md)
- [Agent instructions](CLAUDE.md)
- [Contributing](docs/CONTRIBUTING.md)

## Licence

The project licence has not yet been selected. Add a root `LICENSE` file before distributing the project. Review the licences of all direct and transitive dependencies separately.
