# Agent and AI Contributor Instructions

These instructions apply to AI coding agents and automated contributors working in this repository. Human contributors should also follow the project documentation and contribution guidelines.

## 1. Read before changing

Before making changes:

1. Read `README.md` and `docs/CONTRIBUTING.md`.
2. Read the relevant documents under `docs/`.
3. Inspect the existing source code, tests and project conventions.
4. Identify the current implementation and limitations.
5. Prefer a small, reviewable change over a broad rewrite.

Do not assume a planned feature has already been implemented. Verify the repository state.

## 2. Architecture rules

- Preserve the modular-monolith approach unless an accepted architecture decision changes it.
- Keep domain logic independent of Axum, SQLx, Valkey, LiveKit and other infrastructure details.
- Application code coordinates use cases and depends on abstractions for external capabilities.
- Infrastructure implements application-defined ports/interfaces.
- HTTP and WebSocket adapters translate protocol input into application operations.
- Compose concrete dependencies at application startup/bootstrap.
- Avoid circular dependencies and unnecessary abstraction layers.
- Do not introduce microservices, Kubernetes, sharding or additional brokers without a demonstrated need and a documented decision.

## 3. Backend rules

- Use Rust's type system to represent valid states and domain concepts.
- Return structured errors; do not expose internal error details to clients.
- Use parameterized database queries.
- Keep database transactions short and explicit.
- Enforce authorization on the server for every protected operation.
- Use bounded channels, queues, concurrency and request sizes.
- Avoid unbounded buffering and loading entire files into memory.
- Use cursor-based pagination for potentially large collections.
- Use UUIDv7 for suitable entity identifiers, but never treat IDs as authorization secrets or ordering guarantees.
- Use explicit per-conversation/channel sequence numbers where ordered message streams require them.
- Make retryable write operations idempotent where appropriate.
- Do not use unsafe Rust unless there is a demonstrated need, documented justification and review.

## 4. Frontend rules

- Use TypeScript with strict checking.
- Keep feature-specific UI, hooks and API interactions within the relevant feature module.
- Use TanStack Query for remote/server state and Zustand for appropriate local UI state.
- Avoid duplicating server state in global stores.
- Validate untrusted input at boundaries; frontend validation does not replace backend validation.
- Keep components focused and accessible.
- Avoid unnecessary rerenders and unbounded message-list rendering; use virtualization when justified by the UI workload.
- Keep web and desktop business logic shared where practical. Put platform-specific functionality behind narrow adapters.

## 5. Security rules

- Never invent or implement cryptographic algorithms.
- Use established, reviewed cryptographic protocols and maintained implementations.
- Never commit secrets, tokens, private keys, credentials or real user data.
- Do not log passwords, session tokens, encryption keys or private message contents.
- Never trust the frontend to enforce permissions.
- Protect against IDOR/BOLA, CSRF, XSS, SSRF, injection, replay and resource-exhaustion risks.
- Apply file size limits and inspect file content; do not trust a client-provided MIME type alone.
- Keep security-sensitive defaults restrictive.
- Do not claim a feature is end-to-end encrypted until its full key lifecycle and threat model have been implemented and reviewed.

## 6. Testing requirements

- Add or update tests for every meaningful behavior change.
- Backend changes should include suitable unit, integration or API tests.
- Frontend changes should include suitable unit, component or integration tests.
- User-critical workflows should have end-to-end coverage where practical.
- Include negative cases: unauthorized access, invalid input, duplicate requests, network failures and retry behavior.
- Do not weaken or delete a test merely to make a change pass. Explain and justify any changed expectation.
- Run relevant formatters, linters, type checks and tests before reporting completion.
- Report exactly what was run and any checks that could not be run.

## 7. Dependencies and licensing

- Prefer free and open-source dependencies that can be self-hosted.
- Review licence, maintenance status, security advisories, platform support and transitive dependencies before adding a package.
- Do not introduce mandatory paid SaaS services or paid API dependencies without explicit approval.
- Update dependency and third-party licence records when dependencies change.
- Do not assume a free tier is suitable for production or unlimited use.

## 8. Database and protocol changes

- Use versioned migrations.
- Consider existing data and rollback implications before changing schemas.
- Add constraints and indexes that support actual invariants and query patterns.
- Keep API and WebSocket contracts explicit and versioned where appropriate.
- Update `docs/DATABASE.md` or `docs/API_AND_REALTIME.md` when their documented contracts change.
- Consider compatibility between old and new clients during protocol changes.

## 9. Documentation and completion report

Update documentation when a change affects architecture, configuration, data models, APIs, realtime events, security, tests or deployment.

When finishing a task, summarize:
1. What changed.
2. Important design decisions.
3. Files affected.
4. Tests and checks executed, with results.
5. Known limitations or follow-up work.

Never claim that code was built, tested, deployed or verified unless that action actually succeeded.
