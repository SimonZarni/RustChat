//! Composition root: builds concrete dependencies and runs the server.

use std::{net::SocketAddr, sync::Arc};

use rustchat_application::{
    auth::{AuthConfig, AuthService},
    error::AppError,
    ratelimit::RateLimitConfig,
};
use rustchat_http::{AppState, HttpConfig, router};
use rustchat_infrastructure::{
    postgres::{self, PgSessionRepository, PgUserRepository},
    ratelimit::InMemoryRateLimiter,
    security::{Argon2Hasher, OsTokenService, SystemClock},
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

    let auth = AuthService::new(
        Arc::new(PgUserRepository::new(pool.clone())),
        Arc::new(PgSessionRepository::new(pool)),
        Arc::new(Argon2Hasher::new()?),
        Arc::new(OsTokenService),
        Arc::new(SystemClock),
        // Per-process limits; see InMemoryRateLimiter for the multi-instance caveat.
        Arc::new(InMemoryRateLimiter::new()),
        AuthConfig {
            registration: config.registration,
            session_ttl: SESSION_TTL,
            rate_limits: RateLimitConfig::default(),
        },
    );
    let state = AppState::new(
        Arc::new(auth),
        HttpConfig {
            allowed_origins: config.allowed_origins,
            cookie_secure: config.cookie_secure,
            session_ttl: SESSION_TTL,
            client_ip: config.client_ip,
        },
    );

    let listener = TcpListener::bind(config.bind_addr).await?;
    tracing::info!(addr = %listener.local_addr()?, "rustchat server listening");
    // Connection info exposes the peer address used for rate limiting.
    axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown::signal())
    .await?;
    Ok(())
}
