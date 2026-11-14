use rustchat_domain::identity::{PasswordError, UsernameError};

/// Errors returned by application use cases. `Internal` carries a
/// diagnostic for server-side logs only and must never reach clients.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("invalid username: {0}")]
    InvalidUsername(#[from] UsernameError),
    #[error("invalid password: {0}")]
    InvalidPassword(#[from] PasswordError),
    #[error("username is already taken")]
    UsernameTaken,
    #[error("registration is closed")]
    RegistrationClosed,
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("not authenticated")]
    Unauthenticated,
    #[error("not found")]
    NotFound,
    #[error("internal error: {0}")]
    Internal(String),
}

impl AppError {
    pub fn internal(error: impl std::fmt::Display) -> Self {
        Self::Internal(error.to_string())
    }
}
