use rustchat_server::{
    bootstrap,
    config::{Config, DatabaseUrl},
};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        None | Some("serve") => bootstrap::run(Config::from_env()?).await,
        Some("migrate") => {
            bootstrap::migrate(&DatabaseUrl::from_env()?).await?;
            tracing::info!("migrations applied");
            Ok(())
        }
        Some(other) => {
            eprintln!("unknown command `{other}`; expected `serve` (default) or `migrate`");
            std::process::exit(2);
        }
    }
}
