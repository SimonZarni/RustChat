//! Composition root: builds concrete dependencies and runs the server.

use std::{net::SocketAddr, sync::Arc};

use rustchat_application::{
    attachments::{AttachmentConfig, AttachmentService},
    auth::{AuthConfig, AuthService, SessionLifecycle},
    communities::{CommunityService, CommunityServiceConfig},
    conversations::{ConversationConfig, ConversationService},
    error::AppError,
    messages::{MessageConfig, MessageService},
    notifications::NotificationService,
    ratelimit::ApiRateLimiter,
    social::SocialService,
};
use rustchat_http::{AppEvents, AppState, HttpConfig, cluster, router};
use rustchat_infrastructure::{
    attachments::S3AttachmentStorage,
    cluster::{PgEventBus, PgPresenceStore},
    mfa::AesGcmTotpEngine,
    postgres::{
        self, PgAttachmentRepository, PgCommunityRepository, PgConversationRepository,
        PgMessageRepository, PgMfaRepository, PgNotificationRepository, PgOutboxRepository,
        PgSessionRepository, PgSocialRepository, PgUserRepository,
    },
    ratelimit::{InMemoryRateLimiter, PgRateLimiter},
    security::{Argon2Hasher, OsTokenService, SystemClock},
    web_push::VapidWebPush,
};
use tokio::net::TcpListener;

use crate::{
    config::{Config, DatabaseUrl},
    shutdown,
};

/// Absolute lifetime of a login session.
const SESSION_TTL: time::Duration = time::Duration::days(30);

/// Apply pending database migrations, then return.
pub async fn migrate(database_url: &DatabaseUrl) -> Result<(), AppError> {
    let pool = postgres::connect(database_url.expose(), 2).await?;
    postgres::migrate(&pool).await?;
    pool.close().await;
    Ok(())
}

pub async fn run(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    let pool = postgres::connect(config.database_url.expose(), config.db_max_connections).await?;
    let (event_sender, _) = tokio::sync::broadcast::channel(512);

    let users = Arc::new(PgUserRepository::new(pool.clone()));
    let auth = AuthService::new(
        users.clone(),
        Arc::new(PgSessionRepository::new(pool.clone())),
        Arc::new(Argon2Hasher::new()?),
        Arc::new(OsTokenService),
        Arc::new(SystemClock),
        Arc::new(PgRateLimiter::new(pool.clone())),
        AuthConfig {
            registration: config.registration,
            session_ttl: SESSION_TTL,
            rate_limits: config.rate_limits,
        },
    )
    .with_session_lifecycle(SessionLifecycle {
        idle_timeout: config.session_idle_timeout,
        ..SessionLifecycle::default()
    })
    .with_instance_admins(config.admin_user_ids.iter().copied());
    let auth = if let Some(email) = config.email.as_ref() {
        auth.with_email_delivery(
            Arc::new(rustchat_infrastructure::email::SmtpEmailDelivery::new(
                &email.host,
                email.port,
                email.username.as_deref(),
                email.password.as_deref(),
                &email.from,
            )?),
            email.public_url.clone(),
        )
    } else {
        auth
    };
    // Without an encryption key the server cannot protect TOTP secrets, so MFA stays off.
    let auth = match config.mfa.as_ref() {
        Some(mfa) => auth.with_mfa(
            Arc::new(PgMfaRepository::new(pool.clone())),
            Arc::new(AesGcmTotpEngine::new(mfa.key.bytes(), mfa.issuer.clone())),
        ),
        None => auth,
    };
    let conversations = Arc::new(ConversationService::new(
        users,
        Arc::new(PgConversationRepository::new(pool.clone())),
        ConversationConfig {
            max_members: config.group_member_limit,
            max_page_size: 100,
        },
    ));
    let messages = Arc::new(MessageService::new(
        Arc::new(PgMessageRepository::new(pool.clone())),
        MessageConfig { max_page_size: 100 },
    ));
    let communities = Arc::new(CommunityService::new(
        Arc::new(PgCommunityRepository::new(pool.clone())),
        Arc::new(OsTokenService),
        Arc::new(SystemClock),
        CommunityServiceConfig {
            max_invite_ttl: time::Duration::days(30),
        },
    ));
    let (typing_sender, _) = tokio::sync::broadcast::channel(256);
    let (read_cursor_sender, _) = tokio::sync::broadcast::channel(256);
    let (reaction_sender, _) = tokio::sync::broadcast::channel(256);
    let (delivery_cursor_sender, _) = tokio::sync::broadcast::channel(256);
    let (presence_sender, _) = tokio::sync::broadcast::channel(256);
    let (community_sender, _) = tokio::sync::broadcast::channel(256);
    let (notification_sender, _) = tokio::sync::broadcast::channel(512);
    // Several instances can share one database: events, outbox dispatch and presence
    // coordinate through it. A random id tells an instance's own events apart.
    let instance_id = uuid::Uuid::now_v7();
    let events = AppEvents {
        messages: event_sender.clone(),
        typing: typing_sender,
        read_cursors: read_cursor_sender,
        reactions: reaction_sender,
        delivery_cursors: delivery_cursor_sender,
        presence: presence_sender,
        community: community_sender,
    };
    let outbox = Arc::new(PgOutboxRepository::new(pool.clone()));
    let (resync_sender, _) = tokio::sync::broadcast::channel(16);
    let bus = PgEventBus::start(
        pool.clone(),
        instance_id,
        cluster::remote_event_handler(events.clone(), outbox.clone(), notification_sender.clone()),
        cluster::gap_handler(resync_sender.clone()),
    );
    let presence_store = Arc::new(PgPresenceStore::new(pool.clone(), instance_id));
    let notifications =
        NotificationService::new(Arc::new(PgNotificationRepository::new(pool.clone())));
    let notifications = if let Some(push) = config.web_push.as_ref() {
        Arc::new(notifications.with_push_delivery(Arc::new(VapidWebPush::new(
            push.public_key.clone(),
            push.private_key_pem.clone(),
            push.subject.clone(),
        )?)))
    } else {
        Arc::new(notifications)
    };

    let state = AppState::new(
        Arc::new(auth),
        conversations,
        messages,
        communities,
        events,
        HttpConfig {
            allowed_origins: config.allowed_origins,
            cookie_secure: config.cookie_secure,
            session_ttl: SESSION_TTL,
            client_ip: config.client_ip,
        },
    )
    .with_social(Arc::new(SocialService::new(Arc::new(
        PgSocialRepository::new(pool.clone()),
    ))))
    .with_notifications(notifications)
    .with_notification_events(notification_sender)
    // General API budgets are per server process (in memory, bounded); the stricter
    // authentication limits above are shared through PostgreSQL.
    .with_cluster(bus.clone(), presence_store.clone(), resync_sender)
    .with_api_rate_limiter(Arc::new(ApiRateLimiter::new(
        Arc::new(InMemoryRateLimiter::new()),
        Arc::new(SystemClock),
        config.api_rate_limits,
    )))
    .with_readiness_probe({
        let pool = pool.clone();
        move || {
            let pool = pool.clone();
            async move { postgres::ready(&pool).await }
        }
    });
    let attachment_service = config.object_storage.as_ref().map(|storage| {
        Arc::new(AttachmentService::new(
            Arc::new(PgAttachmentRepository::new(pool.clone())),
            Arc::new(S3AttachmentStorage::new(
                &storage.endpoint,
                &storage.region,
                &storage.access_key,
                &storage.secret_key,
                &storage.bucket,
            )),
            AttachmentConfig {
                max_file_bytes: config.attachment_max_file_bytes,
                quota_bytes: config.attachment_quota_bytes,
            },
        ))
    });
    let state = match attachment_service.as_ref() {
        Some(service) => state.with_attachments(service.clone()),
        None => state,
    };

    let listener = TcpListener::bind(config.bind_addr).await?;
    tracing::info!(addr = %listener.local_addr()?, "rustchat server listening");
    // Connection info exposes the peer address used for rate limiting.
    // Claimed with a lease, so instances sharing the database each dispatch different events.
    let publisher = cluster::spawn_outbox_dispatcher(outbox, event_sender, bus);
    let heartbeat = cluster::spawn_presence_heartbeat(presence_store);
    let attachment_cleanup = attachment_service.map(spawn_attachment_cleanup);
    let serve_result = axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown::signal())
    .await;
    publisher.abort();
    heartbeat.abort();
    if let Some(cleanup) = attachment_cleanup {
        cleanup.abort();
    }
    serve_result?;
    Ok(())
}

fn spawn_attachment_cleanup(service: Arc<AttachmentService>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            match service.cleanup_expired_uploads().await {
                Ok(removed) if removed > 0 => {
                    tracing::info!(removed, "expired attachment uploads cleaned")
                }
                Ok(_) => {}
                Err(error) => tracing::warn!(%error, "expired attachment cleanup pass failed"),
            }
        }
    })
}
