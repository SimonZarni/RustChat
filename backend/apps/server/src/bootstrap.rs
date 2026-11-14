//! Composition root: builds concrete dependencies and runs the server.

use crate::{config::Config, http, shutdown};
use tokio::net::TcpListener;

pub async fn run(config: Config) -> std::io::Result<()> {
    let listener = TcpListener::bind(config.bind_addr).await?;
    tracing::info!(addr = %listener.local_addr()?, "rustchat server listening");
    axum::serve(listener, http::router())
        .with_graceful_shutdown(shutdown::signal())
        .await
}
