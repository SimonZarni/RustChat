//! Composition root: builds concrete dependencies and runs the server.

use std::sync::Arc;

use rustchat_application::{
    auth::{AuthConfig, AuthService},
    error::AppError,
};
use rustchat_http::{AppState, HttpConfig, router};
use rustchat_infrastructure::{
    postgres::{self, PgSessionRepository, PgUserRepository},
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
        AuthConfig {
            registration: config.registration,
            session_ttl: SESSION_TTL,
        },
    );
    let state = AppState::new(
        Arc::new(auth),
        HttpConfig {
            allowed_origins: config.allowed_origins,
            cookie_secure: config.cookie_secure,
            session_ttl: SESSION_TTL,
        },
    );

    let listener = TcpListener::bind(config.bind_addr).await?;
    tracing::info!(addr = %listener.local_addr()?, "rustchat server listening");
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown::signal())
        .await?;
    Ok(())
}
