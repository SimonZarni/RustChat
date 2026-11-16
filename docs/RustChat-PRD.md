# RustChat --- Product Requirements Document (PRD)

**Document status:** Draft\
**Version:** 0.1\
**Product:** RustChat\
**Last updated:** 13 November 2026\
**Audience:** Product, engineering, design, security, and self-hosting
contributors

------------------------------------------------------------------------

## 1. Product overview

### 1.1 Product vision

RustChat is a modern, open-source, self-hostable communication platform
for individuals and communities. It combines direct messaging, group
conversations, community servers, text channels, voice/video
communication, file sharing, and developer integrations in one product.

RustChat should provide a polished, responsive user experience while
giving operators control over their data and deployment. Its backend
will be written primarily in Rust, with a shared web and desktop
frontend.

### 1.2 Problem statement

Many communication platforms depend on centrally operated services,
impose limits on customization or data ownership, or are difficult for
independent operators to deploy and maintain. RustChat aims to offer a
capable alternative that can be independently hosted without requiring a
mandatory third-party application service.

### 1.3 Product principles

1.  **Self-hosting first:** A person or organization can operate its own
    RustChat instance.
2.  **Privacy and security by design:** Access control, safe defaults,
    and careful handling of user data are foundational.
3.  **Reliable communication:** Messages must be persisted reliably and
    synchronized after reconnects.
4.  **Performance with measured trade-offs:** Bound resource usage and
    profile before optimizing.
5.  **Modular, maintainable engineering:** Start with a modular monolith
    rather than premature microservices.
6.  **Progressive delivery:** Build a useful core before advanced
    features.
7.  **Open standards and replaceable infrastructure:** Prefer documented
    protocols and interfaces where practical.
8.  **Accessible, consistent UX:** Support keyboard navigation,
    responsive layouts, and assistive technologies.

### 1.4 Target users

-   Individuals who want private direct and group communication.
-   Friends, gaming groups, clubs, and online communities.
-   Small teams and organizations that want control over their
    communication data.
-   Developers and system administrators who want to self-host or
    integrate a communication platform.

### 1.5 Product goals

-   Deliver dependable direct and group messaging.
-   Provide community/server spaces with channels and configurable
    permissions.
-   Support real-time updates across multiple connected devices.
-   Allow instance owners to deploy, configure, back up, and upgrade
    their own installations.
-   Provide a secure foundation for future end-to-end encryption and
    voice/video.
-   Offer web and desktop clients from a shared frontend codebase.

### 1.6 Non-goals for the initial release

-   Federation between independently operated RustChat instances.
-   A public plugin marketplace.
-   A custom voice/video SFU implementation.
-   Native Android and iOS applications.
-   Full E2EE across every feature at initial launch.
-   Microservices or Kubernetes as a deployment requirement.
-   Reproducing every feature of any single existing communication
    product.

------------------------------------------------------------------------

## 2. Product scope and release strategy

Features are assigned to delivery phases. A feature appearing in this
PRD does not mean it is part of the MVP.

  -----------------------------------------------------------------------
  Phase                   Scope                   Release intent
  ----------------------- ----------------------- -----------------------
  Phase 1 --- MVP         Identity, direct/group  A useful, secure core
                          messaging, communities, communication product
                          text channels, roles,   
                          permissions, reliable   
                          realtime sync           

  Phase 2 --- V1          Attachments,            A practical community
                          friends/blocking,       platform
                          notifications,          
                          moderation, voice,      
                          desktop polish          

  Phase 3 --- V2          E2EE, video/screen      Advanced privacy and
                          sharing,                collaboration
                          threads/forums,         
                          advanced search,        
                          bots/webhooks           

  Phase 4 --- Long term   Mobile apps,            Broader platform
                          federation, plugin      ecosystem
                          ecosystem, advanced     
                          analytics and           
                          integrations            
  -----------------------------------------------------------------------

The release plan may change after technical validation, usability
testing, and operational testing. Dependencies and security requirements
take precedence over a calendar estimate.

------------------------------------------------------------------------

## 3. User types and permissions

### 3.1 Instance roles

An instance may have the following administrative capabilities:

-   **Instance owner:** Controls the installation and has ultimate
    administrative authority.
-   **Instance administrator:** Manages instance configuration, users,
    policies, and operational settings.
-   **Instance moderator/support operator:** Handles reports or user
    conduct where this capability is enabled.
-   **Registered user:** Uses the communication features permitted by
    the instance.

Instance administration must be distinct from permissions inside an
individual community.

### 3.2 Community roles

A community can define custom roles. Typical default roles are:

-   **Owner:** Full control of the community and ownership transfer.
-   **Administrator:** Manages community configuration, roles, channels,
    and members within granted authority.
-   **Moderator:** Handles member and content moderation.
-   **Member:** Participates in permitted channels and conversations.
-   **Restricted member:** Has limited access due to community rules or
    moderation actions.

### 3.3 Permission requirements

-   Permissions must be enforced by the backend, not merely hidden in
    the UI.
-   Users may have multiple roles.
-   Role hierarchy must prevent lower-privilege users from managing
    higher-privilege roles.
-   Channel-level overrides may grant or deny specific permissions.
-   The system must define how conflicting grants and denies are
    resolved.
-   Permission changes must take effect for active sessions and
    WebSocket subscriptions.
-   Sensitive administrative actions should be recorded in an audit log.
-   Authorization must be checked for every protected resource and
    operation.

------------------------------------------------------------------------

## 4. Functional requirements

Priority labels:

-   **P0:** Required for MVP.
-   **P1:** Important for V1.
-   **P2:** Advanced or later release.
-   **P3:** Long-term or optional.

### 4.1 Accounts and authentication

**Priority:** P0, with advanced account protection in P1.

Requirements:

-   Register an account using the instance's configured registration
    policy.
-   Log in and log out.
-   Verify email when email verification is enabled.
-   Request and complete password reset.
-   Change password while authenticated.
-   View and revoke active sessions.
-   View registered devices and manage device sessions.
-   Edit username, display name, avatar, and profile information.
-   Configure basic privacy and discoverability settings.
-   Apply login rate limits and abuse protections.
-   Support account deactivation and deletion according to instance
    policy.
-   Provide MFA using TOTP and/or WebAuthn in a later security
    milestone.
-   Support recovery procedures that do not silently weaken account
    security.

Acceptance criteria:

-   Invalid credentials do not reveal whether a particular account
    exists.
-   Passwords are never stored in plaintext.
-   Session revocation prevents the revoked session from continuing to
    access protected resources.
-   Password reset tokens are short-lived, single-use, and stored
    safely.
-   Authentication and authorization failures are tested.

### 4.2 Profiles, friends, and presence

**Priority:** Basic profiles P0; friends and advanced presence P1.

Requirements:

-   View user profiles permitted by privacy settings.
-   Search for users by supported identifiers.
-   Send, accept, reject, and cancel friend requests.
-   Remove friends.
-   Block and unblock users.
-   View incoming and outgoing friend requests.
-   Show online, idle, do-not-disturb, and offline states where enabled.
-   Allow users to set a custom status.
-   Configure who can see presence and activity information.
-   Avoid exposing precise activity or last-active information beyond
    the user's settings.

Acceptance criteria:

-   Blocked users cannot bypass restrictions through alternate
    conversation entry points.
-   Presence is treated as ephemeral state and is not used as a
    substitute for durable account data.
-   Privacy preferences are enforced by server-side responses and
    realtime events.

### 4.3 Direct messages and group conversations

**Priority:** P0.

Requirements:

-   Start a one-to-one direct message.
-   Create group conversations and manage participants.
-   Send and receive text messages in realtime.
-   Edit and delete messages subject to permissions and product policy.
-   Reply to a message.
-   React to messages with emoji.
-   Display message timestamps and sender information.
-   Show sending, sent, failed, and read states where applicable.
-   Show typing indicators.
-   Track unread messages and read cursors.
-   Load message history using cursor-based pagination.
-   Recover missed messages after reconnect.
-   Search messages in later phases.
-   Support message drafts and local unsent state.
-   Support mentions in group conversations where applicable.
-   Mute or archive conversations in a later phase.
-   Report messages or users where moderation/reporting is enabled.

Acceptance criteria:

-   A successfully accepted message is durably persisted before it is
    reported as committed to the sender.
-   Retrying the same client message identifier does not create
    duplicate messages.
-   Reusing an idempotency identifier with a different payload returns a
    conflict.
-   Message history is consistently ordered using a server-assigned
    conversation sequence, not client clocks or UUID ordering.
-   Reconnecting clients can request missed events from a durable
    cursor.
-   Users cannot read or mutate conversations they are not authorized to
    access.

### 4.4 Communities / servers

**Priority:** P0.

Requirements:

-   Create a community.
-   Join through an invitation or permitted discovery mechanism.
-   Leave a community.
-   View community members.
-   Manage community name, description, icon, and settings.
-   Invite users using links or codes.
-   Revoke invitations.
-   Configure invitation expiration and usage limits.
-   Transfer ownership.
-   Delete a community subject to confirmation and retention policy.
-   Configure public/private visibility where supported.
-   Maintain an audit trail for important administrative actions.

Acceptance criteria:

-   Only authorized roles can create or revoke invitations and change
    community settings.
-   Expired, revoked, or exhausted invitations cannot be used.
-   Ownership transfer requires explicit confirmation and leaves a
    traceable audit event.

### 4.5 Channels and categories

**Priority:** P0 for text channels; voice channels P1.

Requirements:

-   Create, rename, reorder, and delete text channels.
-   Organize channels into categories.
-   Set channel descriptions and topics.
-   Configure public or restricted channel access.
-   Support announcement/read-only channels in a later phase.
-   Configure channel-specific permission overrides.
-   Allow users to mute channels.
-   Support channel archiving where appropriate.
-   Add voice channels in the voice milestone.

Acceptance criteria:

-   Channel visibility is checked server-side on listing, subscription,
    history retrieval, and message submission.
-   Permission changes remove unauthorized realtime subscriptions.
-   Channel ordering is stable and persists across sessions.

### 4.6 Roles and permissions

**Priority:** P0.

Requirements:

-   Create, edit, reorder, and delete custom roles.
-   Assign and remove roles from members.
-   Configure role names and optional display colors.
-   Define community-wide permissions.
-   Define channel-level permission overrides.
-   Support member management, message management, channel management,
    invitation management, and administrative permissions.
-   Enforce role hierarchy.
-   Prevent administrators from granting permissions they are not
    allowed to delegate.

Acceptance criteria:

-   Every protected operation checks the effective permission on the
    backend.
-   Permission evaluation has documented precedence rules.
-   Permission changes invalidate or refresh relevant cached
    authorization state.
-   Tests cover privilege escalation and conflicting permission
    overrides.

### 4.7 Realtime communication and synchronization

**Priority:** P0.

Requirements:

-   Authenticated WebSocket connections.
-   Realtime message delivery.
-   Delivery acknowledgements.
-   Typing indicators and presence updates.
-   Realtime reactions, edits, and deletions.
-   Member and channel updates.
-   Read cursor updates.
-   Heartbeats and connection health.
-   Reconnection with cursor-based synchronization.
-   Multi-device session support.
-   Duplicate event detection.
-   Bounded queues and slow-client handling.
-   Rate limits and payload-size limits.

Acceptance criteria:

-   WebSocket events are authorized independently of the initial
    connection.
-   A reconnect does not silently skip durable messages.
-   Duplicate delivery does not cause duplicate user-visible messages or
    state mutations.
-   Slow clients cannot cause unbounded memory growth.
-   The server can shut down gracefully and allow clients to reconnect.

### 4.8 File and attachment sharing

**Priority:** P1.

Requirements:

-   Upload images, videos, audio, and documents within
    instance-configured limits.
-   Support multiple attachments per message.
-   Show upload progress and allow cancellation.
-   Display safe previews for supported formats.
-   Download attachments only when authorized.
-   Store attachment metadata and ownership in PostgreSQL.
-   Store file bytes in S3-compatible object storage.
-   Support quotas and per-file limits.
-   Validate actual content rather than trusting the supplied MIME type.
-   Provide malware scanning and safe preview generation where
    configured.
-   Delete or expire attachments according to retention policy.

Acceptance criteria:

-   Upload authorization is checked before issuing a short-lived upload
    URL.
-   The backend verifies the completed object and records trusted
    metadata.
-   Private attachments cannot be retrieved by guessing an object key.
-   Upload and download paths stream data rather than loading whole
    files into application memory.

### 4.9 Voice communication

**Priority:** P1.

Requirements:

-   One-to-one and group voice calls.
-   Join and leave voice channels.
-   Show participants and speaking indicators.
-   Mute and unmute microphone.
-   Select audio input/output devices.
-   Adjust local participant volume.
-   Support push-to-talk where practical.
-   Reconnect after transient network failure.
-   Enforce channel and participant permissions.
-   Show connection quality where feasible.

Implementation constraint:

-   Use a self-hosted LiveKit SFU initially.
-   RustChat authorizes room access and issues short-lived access
    tokens.
-   Do not build a custom SFU as part of the initial product.

Acceptance criteria:

-   Users cannot join rooms without valid authorization.
-   Tokens are short-lived and scoped to the intended room and
    participant.
-   Voice service failure does not corrupt text messaging or other core
    data.

### 4.10 Video and screen sharing

**Priority:** P2.

Requirements:

-   One-to-one and group video calls.
-   Camera enable/disable.
-   Screen or application-window sharing.
-   Participant layout controls.
-   Network quality indicators.
-   Adaptive quality and reconnection behavior.
-   Permission controls for joining and sharing.

Acceptance criteria:

-   Video and screen-sharing authorization is enforced.
-   Resource and participant limits are configurable.
-   Media transport encryption is enabled.
-   Media E2EE is treated as a separate security requirement; encrypted
    transport to an SFU alone is not E2EE.

### 4.11 End-to-end encryption and privacy

**Priority:** P2, implemented as a dedicated security milestone.

Requirements:

-   E2EE for supported one-to-one conversations.
-   Group E2EE with authenticated membership changes.
-   Per-device identity and encryption keys.
-   Device verification and revocation.
-   Forward secrecy and appropriate key rotation.
-   Secure new-device enrollment and recovery design.
-   Encrypted attachments.
-   Encrypted local storage where supported.
-   Clear indicators for encrypted and non-encrypted conversations.
-   Server stores ciphertext and required routing metadata, not message
    decryption keys.
-   Define backup and multi-device synchronization behavior without
    silently weakening encryption.

Acceptance criteria:

-   Security design and threat model are reviewed before implementation.
-   Cryptographic protocols and libraries are selected based on
    established standards, maintenance, platform support, and
    independent review.
-   RustChat does not invent its own cryptographic protocol.
-   Tests cover device addition/removal, group membership changes, key
    rotation, and recovery.
-   The product clearly explains what metadata remains visible to the
    server.

### 4.12 Search and discovery

**Priority:** P2.

Requirements:

-   Search messages the current user is authorized to access.
-   Filter by sender, channel/conversation, and date where supported.
-   Search users by supported identifiers.
-   Search community and channel listings where enabled.
-   Show useful result context without leaking unauthorized content.
-   Support recent searches and clear-search-history controls where
    appropriate.

E2EE constraint:

-   Server-side full-text search cannot inspect E2EE ciphertext. Any
    encrypted-message search must use a separately designed client-side
    or privacy-preserving approach.

### 4.13 Notifications

**Priority:** P1.

Requirements:

-   In-app notifications.
-   Browser notifications where permitted.
-   Desktop notifications.
-   Unread badges.
-   Notifications for direct messages, mentions, friend requests, and
    selected community events.
-   Per-conversation and per-channel notification preferences.
-   Mute controls.
-   Do-not-disturb mode and optional schedules.
-   Avoid notification content leakage on locked or shared devices where
    possible.

Mobile push notifications are deferred until mobile clients are planned.

### 4.14 Moderation and safety

**Priority:** P1.

Requirements:

-   Kick and ban members.
-   Temporary bans and timeouts.
-   Unban members.
-   Delete messages where authorized.
-   Report users and messages.
-   Review and resolve reports.
-   Manage moderator roles.
-   Configure invitation restrictions and slow mode.
-   Apply rate limits and spam controls.
-   Maintain an audit trail for moderation actions.
-   Provide appeal or review processes as an instance/community policy
    decision.
-   Support optional automated moderation in a later phase.

Acceptance criteria:

-   Moderation actions are permission-checked and auditable.
-   Users cannot moderate owners or higher-ranked roles unless
    explicitly permitted by policy.
-   Reports are visible only to authorized reviewers.
-   Audit logs avoid storing passwords, tokens, encryption keys, or
    unnecessary message content.

### 4.15 Threads and forum-style channels

**Priority:** P2.

Requirements:

-   Start a thread from a message.
-   Reply within a thread.
-   Show thread participants and unread state.
-   Notify participants according to preferences.
-   Search threads.
-   Support forum-style posts with optional tags and sorting in a later
    phase.
-   Apply channel permissions to thread operations.

### 4.16 Bots, webhooks, and integrations

**Priority:** P2/P3.

Requirements:

-   Developer-created bot identities.
-   Scoped bot permissions.
-   API tokens with revocation and rotation.
-   Incoming and outgoing webhooks.
-   Signed webhook requests.
-   Retry and delivery status for outgoing webhooks.
-   Rate limits and audit logs.
-   Documented APIs and integration examples.
-   Slash/custom commands where supported.

A public plugin marketplace is not required for the initial releases.

### 4.17 User experience and accessibility

**Priority:** P0 baseline; advanced customization P1/P2.

Requirements:

-   Responsive web interface.
-   Consistent layouts for direct messages, groups, communities, and
    channels.
-   Light and dark themes.
-   Keyboard navigation and shortcuts.
-   Accessible labels and focus states.
-   Screen-reader support for core workflows.
-   Adjustable text size where practical.
-   Clear loading, empty, offline, error, and reconnect states.
-   Message formatting for common text, links, lists, and code.
-   Emoji reactions.
-   Drag-and-drop attachments in supported clients.
-   Avoid excessive motion and provide reduced-motion support.

### 4.18 Desktop application

**Priority:** P1.

Requirements:

-   Windows, macOS, and Linux desktop builds.
-   Reuse the web frontend inside Tauri.
-   Native notifications.
-   Secure credential storage using operating-system facilities.
-   Deep links.
-   File selection and drag-and-drop.
-   Tray behavior and startup options where supported.
-   Update mechanism with integrity verification.
-   Persist window size and position.
-   Do not duplicate business logic between web and desktop.

### 4.19 Mobile applications

**Priority:** P3.

Requirements to consider in a future mobile milestone:

-   Android and iOS clients.
-   Push notifications.
-   Mobile-friendly navigation.
-   Camera and gallery attachments.
-   Secure local credential storage.
-   Biometric unlock where supported.
-   Mobile voice/video.
-   Battery-conscious background synchronization.
-   Deep links and share-sheet integration.

The backend API and authorization model should not depend on a specific
client platform.

### 4.20 Self-hosting and instance administration

**Priority:** P0 deployment baseline; richer admin UI P1.

Requirements:

-   Documented Docker Compose deployment.
-   Configurable domain and HTTPS.
-   Environment-based configuration and generated secrets.
-   PostgreSQL, Valkey, object storage, and optional LiveKit services.
-   Persistent volumes.
-   Database migrations.
-   Health and readiness checks.
-   Graceful shutdown.
-   Backup and restore procedures.
-   Upgrade and rollback guidance.
-   Configurable registration policy and instance limits.
-   Operational logs and metrics.
-   Admin controls for user and instance management.
-   Maintenance mode where practical.

Acceptance criteria:

-   A new operator can deploy using documented steps.
-   Internal database/cache/storage ports are not exposed publicly by
    default.
-   Secrets are not committed to source control or printed in logs.
-   Backups are tested by performing a restore, not merely by checking
    that a backup file exists.
-   The instance remains usable for core text communication if optional
    voice services are unavailable.

### 4.21 Developer platform and APIs

**Priority:** P2.

Requirements:

-   Documented REST API.
-   Documented realtime/WebSocket event protocol.
-   OpenAPI specification for HTTP endpoints.
-   API authentication and scoped access.
-   Versioning and compatibility policy.
-   Rate limiting.
-   Developer and bot credentials.
-   Webhook documentation.
-   Revocation and audit controls.
-   Example integrations and SDKs where justified.

------------------------------------------------------------------------

## 5. Core user journeys

The following journeys should be designed and tested before the
corresponding feature is considered complete.

### 5.1 Create an account and sign in

1.  User opens an instance.
2.  User registers if registration is enabled.
3.  User verifies email if required.
4.  User signs in.
5.  Server creates a revocable session.
6.  User lands in the main application.
7.  User can view and revoke sessions from account settings.

### 5.2 Send a direct message

1.  User finds another user or opens an existing conversation.
2.  Client generates a stable message identifier.
3.  Client submits the message over the supported API.
4.  Server authenticates and authorizes the sender.
5.  Server validates and persists the message and outbox event in one
    transaction.
6.  Server acknowledges the committed message.
7.  Outbox processing publishes the event to authorized recipients.
8.  Recipient clients update their conversation.
9.  Reconnecting clients synchronize from their last durable cursor.

### 5.3 Create a community and channel

1.  User creates a community.
2.  Creator becomes owner.
3.  Owner creates categories and text channels.
4.  Owner configures roles and permissions.
5.  Owner creates an invitation.
6.  Another user joins using the invitation.
7.  Backend checks invitation validity and membership rules.
8.  New member sees only channels they are authorized to access.

### 5.4 Join a voice channel

1.  User opens a voice channel.
2.  Backend verifies membership and voice permissions.
3.  Backend issues a short-lived LiveKit token scoped to the room.
4.  Client connects to the self-hosted LiveKit service.
5.  Client displays participant and connection state.
6.  User leaves or loses connection; server-side room state eventually
    reconciles.

### 5.5 Upload an attachment

1.  Client requests an upload authorization.
2.  Backend validates conversation access, size, and allowed file type
    policy.
3.  Backend issues a short-lived upload URL.
4.  Client uploads directly to object storage.
5.  Backend verifies object metadata and records the attachment.
6.  Client sends a message referencing the attachment.
7.  Recipient downloads only after backend authorization.

------------------------------------------------------------------------

## 6. Non-functional requirements

### 6.1 Security

-   Apply least privilege to users, services, and database roles.
-   Enforce authorization server-side for every protected action.
-   Use TLS for network communication.
-   Hash passwords with Argon2id.
-   Use secure, revocable sessions.
-   Protect against CSRF, XSS, SQL injection, SSRF, replay, WebSocket
    hijacking, IDOR/BOLA, and denial-of-service abuse.
-   Apply rate limits and payload/attachment size limits.
-   Validate all external input.
-   Keep secrets out of logs and source control.
-   Run dependency and vulnerability checks.
-   Maintain a threat model and security review checklist.
-   Never claim E2EE unless the complete protocol and implementation
    provide it.

### 6.2 Reliability and consistency

-   Persist accepted messages before acknowledging durable acceptance.
-   Use database transactions and a transactional outbox for reliable
    event publication.
-   Design for at-least-once delivery and idempotent consumers.
-   Provide reconnect synchronization from durable cursors.
-   Handle process restart and dependency outages.
-   Use bounded retries with backoff.
-   Gracefully shut down and drain work where practical.
-   Provide backup and restore procedures.

### 6.3 Performance and resource use

Initial targets are provisional and must be validated through
benchmarks:

-   Core text interactions should feel responsive under normal network
    conditions.
-   Establish measurable p50, p95, and p99 API and message-delivery
    latency targets before public release.
-   Avoid unbounded queues, caches, payloads, and concurrency.
-   Stream attachments.
-   Use cursor pagination.
-   Avoid N+1 queries.
-   Use bounded database connection pools.
-   Track memory per WebSocket connection and total process memory.
-   Profile before introducing complex optimizations.

### 6.4 Scalability

-   Begin as a modular monolith.
-   Support multiple application instances when deployment needs justify
    it.
-   Keep persistent state in PostgreSQL and object storage.
-   Treat Valkey as a replaceable cache/ephemeral coordination
    component.
-   Keep realtime event contracts explicit and versioned.
-   Introduce a durable event broker only when measured requirements
    justify it.

### 6.5 Accessibility and compatibility

-   Core workflows should be keyboard accessible.
-   Use semantic HTML and accessible names.
-   Respect reduced-motion preferences.
-   Support current stable versions of major desktop browsers at
    release.
-   Publish a supported OS/client matrix for desktop builds.

### 6.6 Observability

-   Structured logs with correlation/request IDs.
-   Metrics for API latency, errors, active WebSockets, queue depth, DB
    pool use, memory, CPU, and background jobs.
-   Distributed tracing where useful.
-   Health and readiness endpoints.
-   Avoid logging message contents, credentials, tokens, or encryption
    keys.

------------------------------------------------------------------------

## 7. Data, privacy, and retention

-   Users must be able to understand what data the instance stores.
-   Instance operators control retention policies within product
    constraints.
-   Account deletion must define how messages, attachments, audit
    records, and backups are handled.
-   Deletion from active storage may not immediately remove historical
    backup copies; this limitation must be documented.
-   Presence and typing indicators are ephemeral.
-   Message retention and legal/compliance requirements are configurable
    where practical.
-   Audit logs must be access-controlled and avoid unnecessary sensitive
    data.
-   E2EE metadata exposure must be documented separately from
    message-content confidentiality.

------------------------------------------------------------------------

## 8. External services and infrastructure boundaries

The initial deployment should be self-hostable and should not require a
RustChat-operated cloud service.

  -----------------------------------------------------------------------
  Capability              Initial choice          Responsibility
  ----------------------- ----------------------- -----------------------
  HTTP and WebSocket API  Axum                    RustChat backend

  Database                PostgreSQL              Durable application
                                                  data

  Cache and ephemeral     Valkey                  Presence, transient
  coordination                                    coordination, cache

  Object storage          Garage, S3-compatible   Attachment bytes

  Voice/video media       LiveKit Community       SFU/media transport
                          Edition, self-hosted    

  Reverse proxy and HTTPS Caddy                   TLS and routing

  Deployment              Docker Compose          Single-instance
                                                  deployment

  Metrics and dashboards  Prometheus + Grafana    Operational visibility

  Error tracking          Optional self-hosted    Error reporting
                          GlitchTip               

  Load testing            k6                      Performance validation
  -----------------------------------------------------------------------

All dependencies must be reviewed for current licensing, maintenance,
security posture, and operational fit before adoption. Optional
integrations must fail independently where possible.

------------------------------------------------------------------------

## 9. API and realtime requirements

### 9.1 HTTP API

-   Use resource-oriented endpoints and consistent response/error
    formats.
-   Publish an OpenAPI specification.
-   Authenticate requests and authorize access to each resource.
-   Version breaking API changes.
-   Use cursor pagination for large collections.
-   Use idempotency keys for operations that may be retried.
-   Avoid exposing internal database models directly as public API
    contracts.

### 9.2 WebSocket protocol

-   Authenticate connections.
-   Validate origin and session state.
-   Define event names, payload schemas, versioning, and error behavior.
-   Enforce per-event authorization.
-   Apply connection, message-size, and rate limits.
-   Use heartbeat and reconnect behavior.
-   Provide event IDs or durable cursors for synchronization.
-   Handle duplicate events safely.
-   Use bounded outbound queues and disconnect or degrade slow consumers
    according to documented policy.

------------------------------------------------------------------------

## 10. Testing and quality gates

### Backend

-   Domain unit tests.
-   Application use-case tests.
-   PostgreSQL integration tests.
-   API and WebSocket integration tests.
-   Authorization and security tests.
-   Migration tests on fresh and upgraded databases.
-   Outbox retry and duplicate-delivery tests.
-   Failure tests for unavailable dependencies and process restarts.
-   Load and soak tests.

### Frontend

-   Unit tests with Vitest.
-   Component tests with React Testing Library.
-   API mocking with MSW.
-   End-to-end tests with Playwright.
-   Accessibility checks for core workflows.
-   Production build and type checking.

### Rust quality checks

-   `cargo fmt --check`
-   `cargo clippy --all-targets --all-features -- -D warnings`
-   `cargo test`
-   `cargo audit`
-   `cargo deny check`

The project must report which checks actually ran. A check must not be
described as passing unless it was executed successfully.

------------------------------------------------------------------------

## 11. MVP definition of done

The MVP is ready for an initial release candidate when all of the
following are true:

-   Users can register/sign in according to instance policy and manage
    sessions.
-   Users can create and use one-to-one and group conversations.
-   Users can send, edit, and delete messages according to permissions.
-   Message history is durable and paginated.
-   Communities, text channels, members, roles, and permissions work.
-   WebSocket updates and reconnect synchronization work.
-   Backend authorization is tested for all core operations.
-   Duplicate message submissions are idempotent.
-   Message persistence and event publication use a reliable outbox
    flow.
-   Basic abuse controls and rate limits exist.
-   Core frontend workflows are responsive and accessible.
-   Automated backend and frontend tests run in CI.
-   Docker Compose deployment works from documented instructions.
-   Configuration, secrets, migrations, health checks, backup, and
    restore instructions exist.
-   No critical or high-severity known security issue remains unresolved
    without an explicit release decision.

Voice, video, E2EE, bots, federation, and native mobile applications are
not MVP blockers unless the product scope is formally changed.

------------------------------------------------------------------------

## 12. Delivery roadmap and exit criteria

### Phase 0 --- Product and engineering foundation

Deliver:

-   Approved PRD and architecture decisions.
-   Repository and workspace setup.
-   Local development environment.
-   CI quality gates.
-   Initial database migration strategy.
-   Authentication and authorization threat model.

Exit criteria: contributors can build, test, and run the minimal
application locally using documented steps.

### Phase 1 --- MVP communication core

Deliver:

-   Accounts and sessions.
-   Profiles.
-   Direct and group conversations.
-   Durable messages and history.
-   Communities and text channels.
-   Roles and permissions.
-   Realtime delivery and reconnect synchronization.
-   Core web interface.
-   Self-hosted development/deployment baseline.

Exit criteria: users can securely communicate in direct/group
conversations and community text channels, including after reconnects.

### Phase 2 --- V1 community platform

Deliver:

-   Attachments using Garage object storage.
-   Friends and blocking.
-   Notifications.
-   Moderation tools.
-   Voice using self-hosted LiveKit.
-   Desktop packaging and native integrations.
-   Operational monitoring and backup/restore validation.

Exit criteria: a community can use RustChat for routine text, file, and
voice communication on a self-hosted instance.

### Phase 3 --- V2 privacy and advanced collaboration

Deliver:

-   E2EE for selected conversation types.
-   Device verification and key lifecycle.
-   Encrypted attachments.
-   Video and screen sharing.
-   Threads and forum-style channels.
-   Advanced search where compatible with encryption.
-   Bots, webhooks, and developer APIs.

Exit criteria: security review, protocol lifecycle tests, and
operational tests support each released advanced feature.

### Phase 4 --- Long-term platform

Potential deliverables:

-   Android and iOS clients.
-   Federation between independent instances.
-   Plugin ecosystem.
-   Advanced analytics and administration.
-   Additional integrations and interoperability.

These features require separate product and security decisions before
implementation.

------------------------------------------------------------------------

## 13. Risks, dependencies, and open decisions

### Key risks

-   **Scope growth:** Too many features can delay a usable MVP.
    Mitigation: enforce phase boundaries.
-   **E2EE complexity:** Device and group-key lifecycle is difficult.
    Mitigation: separate milestone, established protocols, external
    review.
-   **Realtime reliability:** WebSocket delivery can fail or duplicate.
    Mitigation: durable cursors, outbox, idempotency.
-   **Self-hosting complexity:** Operators have different environments.
    Mitigation: supported deployment profile, health checks, tested
    upgrades and backups.
-   **Voice networking:** UDP, NAT, firewall, and TURN configuration can
    be difficult. Mitigation: document network requirements and use
    LiveKit.
-   **File safety:** Uploaded files can be malicious. Mitigation:
    limits, content inspection, scanning, safe serving.
-   **Resource limits:** Small servers can be overwhelmed. Mitigation:
    bounded work, quotas, observability, load tests.

### Resolved decisions

-   Initial account policy: username/password accounts, registration open by
    default and configurable as closed. Optional email supports password
    recovery when SMTP is configured; addresses are verified by an emailed link before recovery uses them. See
    [ADR 0005](adr/0005-username-password-accounts-with-configurable-registration.md)
    [ADR 0010](adr/0010-optional-email-password-recovery.md) and
    [ADR 0012](adr/0012-email-verification.md).
-   Optional TOTP two-step verification with recovery codes is included now;
    see [ADR 0013](adr/0013-totp-two-step-verification.md).
-   Production authentication rate limits use shared PostgreSQL storage,
    while WebSocket fan-out remains single-instance. See
    [ADR 0006](adr/0006-in-process-rate-limiting-behind-a-port.md) and
    [ADR 0011](adr/0011-postgresql-shared-auth-rate-limits.md).

### Remaining decisions

-   Supported PostgreSQL version range; PostgreSQL 18 is used in development
    and integration tests. Valkey is currently unused by the application.
-   Exact attachment size and storage quota defaults.
-   Initial community discovery policy.
-   Supported desktop operating systems for the first desktop release.
-   E2EE protocol/library selection after a focused security evaluation.
-   Operational minimum server sizing after baseline load tests.

### Proposed resolutions (awaiting approval)

These are recommendations for the product owner, not approved decisions.
Nothing below changes the implementation until it is accepted and, where
it is architectural, recorded in an ADR.

| Decision | Recommendation | Reasoning and what would change it |
|---|---|---|
| Supported PostgreSQL versions | Support the two newest major versions (currently 17 and 18) and run CI against both. Reject older majors at startup with a clear message. | Identifiers are generated by the application rather than by PostgreSQL, so a newer major should not be required, but this must be confirmed by running the full suite on 17 (it is only tested on 18 today). Testing two majors gives operators an upgrade window without an open-ended matrix. Revisit when a feature needs a newer major. |
| Attachment limits | Defaults of 25 MiB per file and 2 GiB per user, both configurable by the operator (per-file ceiling 100 MiB). Allow-listed content types, content sniffing rather than trusting the client MIME type, and no inline rendering of active content. | Small self-hosted servers need conservative defaults; operators with more storage can raise them. Confirm with load and storage measurements once Phase 2 object storage exists. |
| Community discovery | Invite-only for the first releases (the current behaviour). No public directory. Consider an opt-in, per-instance directory later. | A directory needs moderation and abuse controls that are out of scope for the MVP, and invite-only is the safest default for self-hosters. |
| Desktop operating systems | First desktop release: Windows 10/11 x64, macOS on Apple silicon and Intel, Linux x64 (a Debian/Ubuntu package and an AppImage). No Windows or Linux ARM builds initially. | These are the common Tauri 2 targets and keep the build and test matrix small. Add platforms on demand with CI coverage. |
| E2EE protocol | Evaluate MLS (RFC 9420) using a maintained Rust implementation such as OpenMLS as the first candidate for group conversations, against a Signal-style double-ratchet design, in a time-boxed security spike. Do not ship or describe anything as end-to-end encrypted before the full key lifecycle, threat model and an external review exist. | MLS is purpose-built for group key agreement and is an IETF standard. The choice needs a focused evaluation (library maturity, device recovery, multi-device, metadata exposure), so this is a decision to schedule, not to make now. |
| Minimum server sizing | Treat 2 vCPU, 4 GiB RAM and 40 GB SSD as a working hypothesis for a small community, and publish figures only after a k6 baseline measures them. | No load tests exist yet. Publishing numbers before measuring would be a guess; define the load profile (users, messages per second, connections) and measure first. |

------------------------------------------------------------------------

## 14. Product success measures

Do not optimize for vanity metrics alone. Evaluate:

-   Successful completion rate of registration and first-message
    journeys.
-   Message persistence and synchronization correctness.
-   Message delivery and API latency at p50/p95/p99.
-   WebSocket reconnect success and missed-event recovery.
-   Crash/error rate and uptime during controlled testing.
-   Memory and CPU usage under representative concurrent connections.
-   Backup restore success.
-   Time and number of steps required for a new operator to deploy.
-   Accessibility and usability findings from real users.
-   Security findings resolved before release.

Numeric service-level objectives should be set after the first
representative benchmark, not invented before the system is measured.

------------------------------------------------------------------------

## 15. Change management

This PRD is the product-level source of truth for scope and expected
behavior.

-   Product-scope changes should update this document.
-   Architecture and implementation decisions belong in the engineering
    documentation and relevant decision records.
-   A feature is not complete merely because its UI exists; it must
    satisfy backend authorization, failure handling, tests, and
    acceptance criteria.
-   Any change that affects security, privacy, data retention,
    encryption, or compatibility requires explicit review.
-   Keep this document focused on product requirements; avoid
    duplicating low-level implementation details that belong in
    architecture/API/database documentation.
