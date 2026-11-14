//! RustChat infrastructure: implementations of application ports.

pub mod postgres;
pub mod security;

/// Embedded versioned migrations (`backend/migrations`).
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");
