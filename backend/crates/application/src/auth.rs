//! Account registration, login and session management use cases.

use std::{net::IpAddr, sync::Arc};

use rustchat_domain::identity::{PlainPassword, Session, SessionId, User, UserId, Username};
use time::{Duration, OffsetDateTime};

use crate::{
    error::AppError,
    ports::{
        Clock, CreateUserOutcome, NewSession, NewUser, PasswordHasher, RateLimitDecision,
        RateLimitRule, RateLimiter, SessionRepository, TokenService, UserRepository,
    },
    ratelimit::{RateLimitConfig, login_ip_key, login_user_key, register_ip_key},
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
    pub rate_limits: RateLimitConfig,
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
    limiter: Arc<dyn RateLimiter>,
    config: AuthConfig,
}

impl AuthService {
    pub fn new(
        users: Arc<dyn UserRepository>,
        sessions: Arc<dyn SessionRepository>,
        hasher: Arc<dyn PasswordHasher>,
        tokens: Arc<dyn TokenService>,
        clock: Arc<dyn Clock>,
        limiter: Arc<dyn RateLimiter>,
        config: AuthConfig,
    ) -> Self {
        Self {
            users,
            sessions,
            hasher,
            tokens,
            clock,
            limiter,
            config,
        }
    }

    /// Fail with `RateLimited` if `key` has exhausted its budget.
    async fn enforce(
        &self,
        key: &str,
        rule: RateLimitRule,
        now: OffsetDateTime,
    ) -> Result<(), AppError> {
        match self.limiter.check(key, rule, now).await? {
            RateLimitDecision::Allowed => Ok(()),
            RateLimitDecision::Limited { retry_after } => Err(AppError::RateLimited {
                // Round up so a client that waits this long is not refused again.
                retry_after_secs: u64::try_from(retry_after.whole_seconds())
                    .unwrap_or(0)
                    .saturating_add(u64::from(retry_after.subsec_nanoseconds() > 0))
                    .max(1),
            }),
        }
    }

    pub async fn register(
        &self,
        client_ip: IpAddr,
        username: &str,
        password: String,
    ) -> Result<User, AppError> {
        if self.config.registration == RegistrationPolicy::Closed {
            return Err(AppError::RegistrationClosed);
        }
        // Every attempt counts, including invalid ones, so bulk probing of
        // username availability is bounded too. Checked before any hashing.
        let now = self.clock.now();
        let rule = self.config.rate_limits.register_per_ip;
        let key = register_ip_key(client_ip);
        self.enforce(&key, rule, now).await?;
        self.limiter.record(&key, rule, now).await?;

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
    ///
    /// Rate limits are enforced before any password work. Every attempt
    /// counts against the client network; failures also count against the
    /// username, and a success clears that username's failures.
    pub async fn login(
        &self,
        client_ip: IpAddr,
        username: &str,
        password: String,
    ) -> Result<LoginResult, AppError> {
        let now = self.clock.now();
        let limits = self.config.rate_limits;
        let ip_key = login_ip_key(client_ip);
        let user_key = login_user_key(username);
        self.enforce(&ip_key, limits.login_per_ip, now).await?;
        self.enforce(&user_key, limits.login_failures_per_user, now)
            .await?;
        self.limiter
            .record(&ip_key, limits.login_per_ip, now)
            .await?;

        let result = self.verify_and_start_session(username, password).await;
        match &result {
            Err(AppError::InvalidCredentials) => {
                self.limiter
                    .record(&user_key, limits.login_failures_per_user, now)
                    .await?;
            }
            Ok(_) => self.limiter.reset(&user_key).await?,
            Err(_) => {}
        }
        result
    }

    async fn verify_and_start_session(
        &self,
        username: &str,
        password: String,
    ) -> Result<LoginResult, AppError> {
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
    use crate::{
        ports::RateLimitRule,
        testing::{Harness, generous_limits},
    };
    use std::net::Ipv4Addr;

    const PASSWORD: &str = "correct horse battery";
    const IP: IpAddr = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7));
    const OTHER_IP: IpAddr = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 9));

    fn rule(max: u32) -> RateLimitRule {
        RateLimitRule {
            max,
            window: Duration::minutes(15),
        }
    }

    fn limits(login_ip: u32, login_user: u32, register_ip: u32) -> RateLimitConfig {
        RateLimitConfig {
            login_per_ip: rule(login_ip),
            login_failures_per_user: rule(login_user),
            register_per_ip: rule(register_ip),
        }
    }

    #[tokio::test]
    async fn register_then_login_then_authenticate() {
        let h = Harness::new(RegistrationPolicy::Open);
        h.auth.register(IP, "Alice", PASSWORD.into()).await.unwrap();
        let login = h.auth.login(IP, "alice", PASSWORD.into()).await.unwrap();
        let ctx = h.auth.authenticate(&login.token).await.unwrap();
        assert_eq!(ctx.user.username.as_str(), "alice");
        assert_eq!(ctx.session_id, login.session.id);
    }

    #[tokio::test]
    async fn register_rejects_duplicate_username_case_insensitively() {
        let h = Harness::new(RegistrationPolicy::Open);
        h.auth.register(IP, "alice", PASSWORD.into()).await.unwrap();
        let err = h
            .auth
            .register(IP, "ALICE", PASSWORD.into())
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::UsernameTaken));
    }

    #[tokio::test]
    async fn register_validates_input() {
        let h = Harness::new(RegistrationPolicy::Open);
        assert!(matches!(
            h.auth.register(IP, "a", PASSWORD.into()).await,
            Err(AppError::InvalidUsername(_))
        ));
        assert!(matches!(
            h.auth.register(IP, "alice", "short".into()).await,
            Err(AppError::InvalidPassword(_))
        ));
    }

    #[tokio::test]
    async fn register_is_refused_when_closed() {
        let h = Harness::new(RegistrationPolicy::Closed);
        let err = h
            .auth
            .register(IP, "alice", PASSWORD.into())
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::RegistrationClosed));
    }

    #[tokio::test]
    async fn wrong_password_and_unknown_user_are_indistinguishable() {
        let h = Harness::new(RegistrationPolicy::Open);
        h.auth.register(IP, "alice", PASSWORD.into()).await.unwrap();
        let wrong = h
            .auth
            .login(IP, "alice", "wrong password!!".into())
            .await
            .unwrap_err();
        let unknown = h
            .auth
            .login(IP, "nobody", PASSWORD.into())
            .await
            .unwrap_err();
        let malformed = h
            .auth
            .login(IP, "bad name!", PASSWORD.into())
            .await
            .unwrap_err();
        for err in [wrong, unknown, malformed] {
            assert!(matches!(err, AppError::InvalidCredentials));
        }
    }

    #[tokio::test]
    async fn unknown_user_login_still_spends_hashing_effort() {
        let h = Harness::new(RegistrationPolicy::Open);
        let _ = h.auth.login(IP, "nobody", PASSWORD.into()).await;
        assert_eq!(h.hasher.dummy_verifications(), 1);
    }

    #[tokio::test]
    async fn each_login_issues_a_distinct_session_and_token() {
        let h = Harness::new(RegistrationPolicy::Open);
        h.auth.register(IP, "alice", PASSWORD.into()).await.unwrap();
        let a = h.auth.login(IP, "alice", PASSWORD.into()).await.unwrap();
        let b = h.auth.login(IP, "alice", PASSWORD.into()).await.unwrap();
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
        h.auth.register(IP, "alice", PASSWORD.into()).await.unwrap();
        let login = h.auth.login(IP, "alice", PASSWORD.into()).await.unwrap();
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
        h.auth.register(IP, "alice", PASSWORD.into()).await.unwrap();
        let a = h.auth.login(IP, "alice", PASSWORD.into()).await.unwrap();
        let b = h.auth.login(IP, "alice", PASSWORD.into()).await.unwrap();
        let ctx_a = h.auth.authenticate(&a.token).await.unwrap();
        h.auth.logout(&ctx_a).await.unwrap();
        assert!(h.auth.authenticate(&a.token).await.is_err());
        assert!(h.auth.authenticate(&b.token).await.is_ok());
    }

    #[tokio::test]
    async fn sessions_can_be_listed_and_revoked() {
        let h = Harness::new(RegistrationPolicy::Open);
        h.auth.register(IP, "alice", PASSWORD.into()).await.unwrap();
        let a = h.auth.login(IP, "alice", PASSWORD.into()).await.unwrap();
        let b = h.auth.login(IP, "alice", PASSWORD.into()).await.unwrap();
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
        h.auth.register(IP, "alice", PASSWORD.into()).await.unwrap();
        h.auth.register(IP, "bobby", PASSWORD.into()).await.unwrap();
        let alice = h.auth.login(IP, "alice", PASSWORD.into()).await.unwrap();
        let bob = h.auth.login(IP, "bobby", PASSWORD.into()).await.unwrap();
        let ctx_bob = h.auth.authenticate(&bob.token).await.unwrap();

        let err = h
            .auth
            .revoke_session(&ctx_bob, alice.session.id)
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::NotFound));
        assert!(h.auth.authenticate(&alice.token).await.is_ok());
    }

    // ---- rate limiting ----

    #[tokio::test]
    async fn repeated_failures_block_the_username_even_with_the_right_password() {
        let h = Harness::with_limits(RegistrationPolicy::Open, limits(100, 3, 100));
        h.auth.register(IP, "alice", PASSWORD.into()).await.unwrap();
        for _ in 0..3 {
            let err = h.auth.login(IP, "alice", "wrong password!!".into()).await;
            assert!(matches!(err, Err(AppError::InvalidCredentials)));
        }
        let work_before = h.hasher.password_work();

        // Even the correct password, and even from a different network.
        for ip in [IP, OTHER_IP] {
            let err = h
                .auth
                .login(ip, "alice", PASSWORD.into())
                .await
                .unwrap_err();
            assert!(matches!(err, AppError::RateLimited { .. }), "{err:?}");
        }
        assert_eq!(
            h.hasher.password_work(),
            work_before,
            "a limited request must not cost any password hashing"
        );
    }

    #[tokio::test]
    async fn a_successful_login_clears_the_username_failures() {
        let h = Harness::with_limits(RegistrationPolicy::Open, limits(100, 3, 100));
        h.auth.register(IP, "alice", PASSWORD.into()).await.unwrap();
        for _ in 0..5 {
            for _ in 0..2 {
                let _ = h.auth.login(IP, "alice", "wrong password!!".into()).await;
            }
            h.auth.login(IP, "alice", PASSWORD.into()).await.unwrap();
        }
    }

    #[tokio::test]
    async fn the_ip_limit_counts_every_attempt_across_usernames() {
        let h = Harness::with_limits(RegistrationPolicy::Open, limits(3, 100, 100));
        h.auth.register(IP, "alice", PASSWORD.into()).await.unwrap();
        for name in ["ghost1", "ghost2", "ghost3"] {
            let err = h.auth.login(IP, name, PASSWORD.into()).await;
            assert!(matches!(err, Err(AppError::InvalidCredentials)));
        }
        let blocked = h
            .auth
            .login(IP, "alice", PASSWORD.into())
            .await
            .unwrap_err();
        assert!(matches!(blocked, AppError::RateLimited { .. }));

        // Another network is unaffected.
        h.auth
            .login(OTHER_IP, "alice", PASSWORD.into())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn successful_logins_also_count_against_the_ip() {
        let h = Harness::with_limits(RegistrationPolicy::Open, limits(2, 100, 100));
        h.auth.register(IP, "alice", PASSWORD.into()).await.unwrap();
        h.auth.login(IP, "alice", PASSWORD.into()).await.unwrap();
        h.auth.login(IP, "alice", PASSWORD.into()).await.unwrap();
        let err = h
            .auth
            .login(IP, "alice", PASSWORD.into())
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::RateLimited { .. }));
    }

    #[tokio::test]
    async fn existing_and_unknown_usernames_are_limited_identically() {
        let h = Harness::with_limits(RegistrationPolicy::Open, limits(100, 2, 100));
        h.auth.register(IP, "alice", PASSWORD.into()).await.unwrap();
        let mut outcomes = Vec::new();
        for name in ["alice", "ghost"] {
            let mut seen = Vec::new();
            for _ in 0..4 {
                let result = h.auth.login(IP, name, "wrong password!!".into()).await;
                seen.push(match result {
                    Err(AppError::InvalidCredentials) => "invalid",
                    Err(AppError::RateLimited { .. }) => "limited",
                    other => panic!("unexpected: {other:?}"),
                });
            }
            outcomes.push(seen);
        }
        assert_eq!(
            outcomes[0], outcomes[1],
            "limiting must not reveal accounts"
        );
        assert_eq!(outcomes[0], ["invalid", "invalid", "limited", "limited"]);
    }

    #[tokio::test]
    async fn username_limits_ignore_case_and_cannot_be_dodged_with_variants() {
        let h = Harness::with_limits(RegistrationPolicy::Open, limits(100, 2, 100));
        for name in ["Alice", "ALICE"] {
            let _ = h.auth.login(IP, name, "wrong password!!".into()).await;
        }
        let err = h.auth.login(IP, "aLiCe", "wrong password!!".into()).await;
        assert!(matches!(err, Err(AppError::RateLimited { .. })));
    }

    #[tokio::test]
    async fn rate_limited_responses_report_when_to_retry() {
        let h = Harness::with_limits(RegistrationPolicy::Open, limits(100, 1, 100));
        let _ = h.auth.login(IP, "ghost", "wrong password!!".into()).await;
        let err = h.auth.login(IP, "ghost", "wrong password!!".into()).await;
        match err {
            Err(AppError::RateLimited { retry_after_secs }) => {
                assert_eq!(retry_after_secs, 15 * 60)
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[tokio::test]
    async fn registration_is_limited_per_ip_and_counts_invalid_attempts() {
        let h = Harness::with_limits(RegistrationPolicy::Open, limits(100, 100, 3));
        h.auth.register(IP, "alice", PASSWORD.into()).await.unwrap();
        assert!(matches!(
            h.auth.register(IP, "x", PASSWORD.into()).await,
            Err(AppError::InvalidUsername(_))
        ));
        assert!(matches!(
            h.auth.register(IP, "alice", PASSWORD.into()).await,
            Err(AppError::UsernameTaken)
        ));
        let work_before = h.hasher.password_work();
        let err = h
            .auth
            .register(IP, "bobby", PASSWORD.into())
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::RateLimited { .. }));
        assert_eq!(h.hasher.password_work(), work_before);

        h.auth
            .register(OTHER_IP, "bobby", PASSWORD.into())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn closed_registration_is_reported_before_rate_limiting() {
        let h = Harness::with_limits(RegistrationPolicy::Closed, limits(100, 100, 0));
        let err = h
            .auth
            .register(IP, "alice", PASSWORD.into())
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::RegistrationClosed));
    }

    #[tokio::test]
    async fn generous_limits_do_not_interfere_with_normal_use() {
        let h = Harness::with_limits(RegistrationPolicy::Open, generous_limits());
        h.auth.register(IP, "alice", PASSWORD.into()).await.unwrap();
        h.auth.login(IP, "alice", PASSWORD.into()).await.unwrap();
    }
}
