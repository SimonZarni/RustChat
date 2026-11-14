//! Account registration, login and session management use cases.

use std::sync::Arc;

use rustchat_domain::identity::{PlainPassword, Session, SessionId, User, UserId, Username};
use time::Duration;

use crate::{
    error::AppError,
    ports::{
        Clock, CreateUserOutcome, NewSession, NewUser, PasswordHasher, SessionRepository,
        TokenService, UserRepository,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrationPolicy {
    Open,
    Closed,
}

#[derive(Debug, Clone, Copy)]
pub struct AuthConfig {
    pub registration: RegistrationPolicy,
    pub session_ttl: Duration,
}

/// The authenticated principal for a request.
#[derive(Debug, Clone)]
pub struct AuthContext {
    pub user: User,
    pub session_id: SessionId,
}

pub struct LoginResult {
    pub user: User,
    /// Secret bearer value to place in the session cookie. Shown once.
    pub token: String,
    pub session: Session,
}

impl std::fmt::Debug for LoginResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginResult")
            .field("user", &self.user)
            .field("token", &"<redacted>")
            .field("session", &self.session)
            .finish()
    }
}

pub struct SessionInfo {
    pub session: Session,
    pub is_current: bool,
}

pub struct AuthService {
    users: Arc<dyn UserRepository>,
    sessions: Arc<dyn SessionRepository>,
    hasher: Arc<dyn PasswordHasher>,
    tokens: Arc<dyn TokenService>,
    clock: Arc<dyn Clock>,
    config: AuthConfig,
}

impl AuthService {
    pub fn new(
        users: Arc<dyn UserRepository>,
        sessions: Arc<dyn SessionRepository>,
        hasher: Arc<dyn PasswordHasher>,
        tokens: Arc<dyn TokenService>,
        clock: Arc<dyn Clock>,
        config: AuthConfig,
    ) -> Self {
        Self {
            users,
            sessions,
            hasher,
            tokens,
            clock,
            config,
        }
    }

    pub async fn register(&self, username: &str, password: String) -> Result<User, AppError> {
        if self.config.registration == RegistrationPolicy::Closed {
            return Err(AppError::RegistrationClosed);
        }
        let username = Username::parse(username)?;
        let password = PlainPassword::parse_new(password)?;
        let password_hash = self.hasher.hash(password).await?;

        let user = User {
            id: UserId::new(),
            username,
            created_at: self.clock.now(),
        };
        let outcome = self
            .users
            .create(NewUser {
                id: user.id,
                username: user.username.clone(),
                password_hash,
                created_at: user.created_at,
            })
            .await?;
        match outcome {
            CreateUserOutcome::Created => Ok(user),
            CreateUserOutcome::UsernameTaken => Err(AppError::UsernameTaken),
        }
    }

    /// Verify credentials and start a new session. Unknown users, malformed
    /// usernames and wrong passwords are indistinguishable to the caller.
    pub async fn login(&self, username: &str, password: String) -> Result<LoginResult, AppError> {
        let Some(password) = PlainPassword::from_login_input(password) else {
            return Err(AppError::InvalidCredentials);
        };
        let credentials = match Username::parse(username) {
            Ok(username) => self.users.find_credentials(&username).await?,
            Err(_) => None,
        };
        let Some(credentials) = credentials else {
            self.hasher.verify_dummy(password).await?;
            return Err(AppError::InvalidCredentials);
        };
        if !self
            .hasher
            .verify(password, credentials.password_hash)
            .await?
        {
            return Err(AppError::InvalidCredentials);
        }

        // A new random token is issued on every login; a client-supplied
        // session identifier is never accepted, preventing fixation.
        let now = self.clock.now();
        let token = self.tokens.generate();
        let session = Session {
            id: SessionId::new(),
            user_id: credentials.user.id,
            created_at: now,
            expires_at: now + self.config.session_ttl,
        };
        self.sessions
            .insert(NewSession {
                session: session.clone(),
                token_digest: token.digest,
            })
            .await?;
        Ok(LoginResult {
            user: credentials.user,
            token: token.raw,
            session,
        })
    }

    /// Resolve a session token to its principal, or `Unauthenticated`.
    pub async fn authenticate(&self, raw_token: &str) -> Result<AuthContext, AppError> {
        let digest = self.tokens.digest(raw_token);
        let now = self.clock.now();
        match self.sessions.find_active(&digest, now).await? {
            Some(active) if !active.session.is_expired_at(now) => Ok(AuthContext {
                user: active.user,
                session_id: active.session.id,
            }),
            _ => Err(AppError::Unauthenticated),
        }
    }

    pub async fn logout(&self, auth: &AuthContext) -> Result<(), AppError> {
        self.sessions
            .revoke(auth.user.id, auth.session_id, self.clock.now())
            .await?;
        Ok(())
    }

    pub async fn list_sessions(&self, auth: &AuthContext) -> Result<Vec<SessionInfo>, AppError> {
        let sessions = self
            .sessions
            .list_active(auth.user.id, self.clock.now())
            .await?;
        Ok(sessions
            .into_iter()
            .map(|session| SessionInfo {
                is_current: session.id == auth.session_id,
                session,
            })
            .collect())
    }

    /// Revoke one of the caller's own sessions. A session that does not exist
    /// or belongs to someone else yields `NotFound`.
    pub async fn revoke_session(
        &self,
        auth: &AuthContext,
        session_id: SessionId,
    ) -> Result<(), AppError> {
        let revoked = self
            .sessions
            .revoke(auth.user.id, session_id, self.clock.now())
            .await?;
        if revoked {
            Ok(())
        } else {
            Err(AppError::NotFound)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Harness;

    const PASSWORD: &str = "correct horse battery";

    #[tokio::test]
    async fn register_then_login_then_authenticate() {
        let h = Harness::new(RegistrationPolicy::Open);
        h.auth.register("Alice", PASSWORD.into()).await.unwrap();
        let login = h.auth.login("alice", PASSWORD.into()).await.unwrap();
        let ctx = h.auth.authenticate(&login.token).await.unwrap();
        assert_eq!(ctx.user.username.as_str(), "alice");
        assert_eq!(ctx.session_id, login.session.id);
    }

    #[tokio::test]
    async fn register_rejects_duplicate_username_case_insensitively() {
        let h = Harness::new(RegistrationPolicy::Open);
        h.auth.register("alice", PASSWORD.into()).await.unwrap();
        let err = h.auth.register("ALICE", PASSWORD.into()).await.unwrap_err();
        assert!(matches!(err, AppError::UsernameTaken));
    }

    #[tokio::test]
    async fn register_validates_input() {
        let h = Harness::new(RegistrationPolicy::Open);
        assert!(matches!(
            h.auth.register("a", PASSWORD.into()).await,
            Err(AppError::InvalidUsername(_))
        ));
        assert!(matches!(
            h.auth.register("alice", "short".into()).await,
            Err(AppError::InvalidPassword(_))
        ));
    }

    #[tokio::test]
    async fn register_is_refused_when_closed() {
        let h = Harness::new(RegistrationPolicy::Closed);
        let err = h.auth.register("alice", PASSWORD.into()).await.unwrap_err();
        assert!(matches!(err, AppError::RegistrationClosed));
    }

    #[tokio::test]
    async fn wrong_password_and_unknown_user_are_indistinguishable() {
        let h = Harness::new(RegistrationPolicy::Open);
        h.auth.register("alice", PASSWORD.into()).await.unwrap();
        let wrong = h
            .auth
            .login("alice", "wrong password!!".into())
            .await
            .unwrap_err();
        let unknown = h.auth.login("nobody", PASSWORD.into()).await.unwrap_err();
        let malformed = h
            .auth
            .login("bad name!", PASSWORD.into())
            .await
            .unwrap_err();
        for err in [wrong, unknown, malformed] {
            assert!(matches!(err, AppError::InvalidCredentials));
        }
    }

    #[tokio::test]
    async fn unknown_user_login_still_spends_hashing_effort() {
        let h = Harness::new(RegistrationPolicy::Open);
        let _ = h.auth.login("nobody", PASSWORD.into()).await;
        assert_eq!(h.hasher.dummy_verifications(), 1);
    }

    #[tokio::test]
    async fn each_login_issues_a_distinct_session_and_token() {
        let h = Harness::new(RegistrationPolicy::Open);
        h.auth.register("alice", PASSWORD.into()).await.unwrap();
        let a = h.auth.login("alice", PASSWORD.into()).await.unwrap();
        let b = h.auth.login("alice", PASSWORD.into()).await.unwrap();
        assert_ne!(a.token, b.token);
        assert_ne!(a.session.id, b.session.id);
    }

    #[tokio::test]
    async fn unknown_token_is_unauthenticated() {
        let h = Harness::new(RegistrationPolicy::Open);
        assert!(matches!(
            h.auth.authenticate("nope").await,
            Err(AppError::Unauthenticated)
        ));
    }

    #[tokio::test]
    async fn session_expires_after_ttl() {
        let h = Harness::new(RegistrationPolicy::Open);
        h.auth.register("alice", PASSWORD.into()).await.unwrap();
        let login = h.auth.login("alice", PASSWORD.into()).await.unwrap();
        h.clock.advance(h.ttl - Duration::seconds(1));
        assert!(h.auth.authenticate(&login.token).await.is_ok());
        h.clock.advance(Duration::seconds(1));
        assert!(matches!(
            h.auth.authenticate(&login.token).await,
            Err(AppError::Unauthenticated)
        ));
    }

    #[tokio::test]
    async fn logout_revokes_only_the_current_session() {
        let h = Harness::new(RegistrationPolicy::Open);
        h.auth.register("alice", PASSWORD.into()).await.unwrap();
        let a = h.auth.login("alice", PASSWORD.into()).await.unwrap();
        let b = h.auth.login("alice", PASSWORD.into()).await.unwrap();
        let ctx_a = h.auth.authenticate(&a.token).await.unwrap();
        h.auth.logout(&ctx_a).await.unwrap();
        assert!(h.auth.authenticate(&a.token).await.is_err());
        assert!(h.auth.authenticate(&b.token).await.is_ok());
    }

    #[tokio::test]
    async fn sessions_can_be_listed_and_revoked() {
        let h = Harness::new(RegistrationPolicy::Open);
        h.auth.register("alice", PASSWORD.into()).await.unwrap();
        let a = h.auth.login("alice", PASSWORD.into()).await.unwrap();
        let b = h.auth.login("alice", PASSWORD.into()).await.unwrap();
        let ctx_a = h.auth.authenticate(&a.token).await.unwrap();

        let listed = h.auth.list_sessions(&ctx_a).await.unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed.iter().filter(|s| s.is_current).count(), 1);

        h.auth.revoke_session(&ctx_a, b.session.id).await.unwrap();
        assert!(h.auth.authenticate(&b.token).await.is_err());
        assert!(matches!(
            h.auth.revoke_session(&ctx_a, b.session.id).await,
            Err(AppError::NotFound)
        ));
    }

    #[tokio::test]
    async fn a_user_cannot_revoke_another_users_session() {
        let h = Harness::new(RegistrationPolicy::Open);
        h.auth.register("alice", PASSWORD.into()).await.unwrap();
        h.auth.register("bobby", PASSWORD.into()).await.unwrap();
        let alice = h.auth.login("alice", PASSWORD.into()).await.unwrap();
        let bob = h.auth.login("bobby", PASSWORD.into()).await.unwrap();
        let ctx_bob = h.auth.authenticate(&bob.token).await.unwrap();

        let err = h
            .auth
            .revoke_session(&ctx_bob, alice.session.id)
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::NotFound));
        assert!(h.auth.authenticate(&alice.token).await.is_ok());
    }
}
