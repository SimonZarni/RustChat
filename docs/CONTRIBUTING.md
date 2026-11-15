# Contributing to RustChat

Thank you for considering a contribution to RustChat. This guide describes the expected workflow and engineering standards. It may be refined as the project becomes active.

## Before contributing

- Read `README.md` and `CLAUDE.md`.
- Read the relevant architecture, database, API/realtime and security documentation.
- Check existing issues and pull requests before starting substantial work.
- For a significant architectural change, discuss the proposal and record the decision as an ADR in [docs/adr](adr/README.md) before or with the implementation.
- Report suspected security vulnerabilities privately, following [.github/SECURITY.md](../.github/SECURITY.md), not in public issues.

## Development principles

- Prefer small, focused changes.
- Preserve established architecture and naming conventions.
- Avoid unrelated refactoring in feature changes.
- Keep domain logic independent from frameworks and infrastructure.
- Validate and authorize operations on the backend.
- Handle errors explicitly; do not silently ignore failures.
- Consider accessibility, performance and resource limits.
- Do not add a dependency when a small, clear implementation using existing dependencies is sufficient.

## Suggested workflow

1. Create a branch for the change.
2. Make the smallest coherent implementation.
3. Add or update tests.
4. Run formatting, linting, type checks and relevant tests.
5. Update documentation where behavior or contracts changed.
6. Review the diff for secrets, generated files, unrelated changes and licence concerns.
7. Open a pull request describing the motivation, implementation, tests and limitations.

## Branch and commit conventions

Suggested branch names:

```text
feature/message-reactions
fix/websocket-reconnect
docs/database-conventions
refactor/auth-session
```

Use clear commit messages. A conventional format is encouraged:

```text
feat(messaging): add message reactions
fix(realtime): handle reconnect cursor
docs(architecture): clarify module boundaries
test(auth): cover session revocation
```

## Testing expectations

### Backend
Run the relevant Rust tests and checks, for example:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

These are the backend quality checks used by CI. Run them from `backend/` (or with `--manifest-path backend/Cargo.toml` from the repository root). PostgreSQL integration tests require a reachable `DATABASE_URL`; see the local development instructions in [README.md](../README.md).

### Frontend
Run the frontend checks from `frontend/`:

- TypeScript type checking
- Oxlint
- Vitest unit and component tests
- Playwright Chromium end-to-end keyboard and accessibility tests
- Production build

These correspond to `pnpm typecheck`, `pnpm lint`, `pnpm test`, `pnpm --filter @rustchat/web test:e2e` and `pnpm build` from `frontend/`. Install the browser once with `pnpm --filter @rustchat/web exec playwright install chromium`. The automated accessibility scan covers the core workspace and conversation dialog in light mode and the workspace in dark mode; browser keyboard coverage also exercises workspace navigation, community join, settings, administration controls, and modal focus restoration. Manually review screen-reader behavior.

Do not report a check as passing unless it was actually executed successfully.

## Security-sensitive changes

Authentication, authorization, cryptography, session handling, uploads and permission changes require particular care.

- Explain the threat or invariant the change addresses.
- Add tests for allowed and denied cases.
- Avoid exposing sensitive data in logs or error responses.
- Request review before introducing a new cryptographic dependency or changing an encryption protocol.

## Pull request description

Include:

- **Purpose:** What problem does this solve?
- **Changes:** What was changed?
- **Design:** Any important decisions or trade-offs.
- **Tests:** Exact checks run and their results.
- **Security:** Any security implications.
- **Documentation:** Which documents were updated?
- **Limitations:** Anything incomplete or requiring follow-up.

## Dependencies and licences

Prefer self-hostable, free and open-source tools. Review the licence and maintenance status of direct and transitive dependencies before adding them. Do not introduce a mandatory paid service without explicit approval.

## Code of conduct

Be respectful, constructive and considerate. Critique code and ideas, not people. A separate `CODE_OF_CONDUCT.md` can be added when the project adopts a specific community standard.
