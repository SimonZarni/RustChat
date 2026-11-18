//! RustChat infrastructure: implementations of application ports.

pub mod attachments;
pub mod cluster;
pub mod email;
pub mod mfa;
pub mod postgres;
pub mod ratelimit;
pub mod security;

/// Embedded versioned migrations (`backend/migrations`). Keep this module
/// source updated when adding migrations so the compile-time embed refreshes.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");
