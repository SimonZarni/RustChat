//! Ports: capabilities the application needs from the outside world.
//! Infrastructure implements them; the application never names a concrete
//! database, hashing library or clock.

use async_trait::async_trait;
use rustchat_domain::identity::{PlainPassword, Session, SessionId, User, UserId, Username};
use time::OffsetDateTime;

use crate::error::AppError;

/// SHA-256 digest of a session token. Only digests are persisted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenDigest(pub [u8; 32]);

/// A freshly generated secret session token and its digest.
pub struct NewToken {
    pub raw: String,
    pub digest: TokenDigest,
}

pub struct NewUser {
    pub id: UserId,
    pub username: Username,
    pub password_hash: String,
    pub created_at: OffsetDateTime,
}

pub struct UserCredentials {
    pub user: User,
    pub password_hash: String,
}

pub struct NewSession {
    pub session: Session,
    pub token_digest: TokenDigest,
}

/// A session joined with its owner, as found by token digest.
pub struct ActiveSession {
    pub session: Session,
    pub user: User,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CreateUserOutcome {
    Created,
    UsernameTaken,
}

#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn create(&self, user: NewUser) -> Result<CreateUserOutcome, AppError>;
    async fn find_credentials(
        &self,
        username: &Username,
    ) -> Result<Option<UserCredentials>, AppError>;
}

#[async_trait]
pub trait SessionRepository: Send + Sync {
    async fn insert(&self, session: NewSession) -> Result<(), AppError>;
    /// Find a session by token digest that is neither revoked nor expired at `now`.
    async fn find_active(
        &self,
        digest: &TokenDigest,
        now: OffsetDateTime,
    ) -> Result<Option<ActiveSession>, AppError>;
    async fn list_active(
        &self,
        user_id: UserId,
        now: OffsetDateTime,
    ) -> Result<Vec<Session>, AppError>;
    /// Revoke a session only if it belongs to `user_id`. Returns whether a
    /// session was revoked.
    async fn revoke(
        &self,
        user_id: UserId,
        session_id: SessionId,
        now: OffsetDateTime,
    ) -> Result<bool, AppError>;
}

#[async_trait]
pub trait PasswordHasher: Send + Sync {
    async fn hash(&self, password: PlainPassword) -> Result<String, AppError>;
    async fn verify(&self, password: PlainPassword, hash: String) -> Result<bool, AppError>;
    /// Spend the same effort as `verify` against a fixed dummy hash, so that
    /// login timing does not reveal whether an account exists.
    async fn verify_dummy(&self, password: PlainPassword) -> Result<(), AppError>;
}

pub trait TokenService: Send + Sync {
    fn generate(&self) -> NewToken;
    fn digest(&self, raw: &str) -> TokenDigest;
}

pub trait Clock: Send + Sync {
    fn now(&self) -> OffsetDateTime;
}
