//! Account registration, login and session management use cases.

use std::{collections::HashSet, net::IpAddr, sync::Arc};

use rustchat_domain::identity::{
    EmailAddress, PlainPassword, ProfilePrivacy, Session, SessionId, User, UserId, UserProfile,
    Username,
};
use time::{Duration, OffsetDateTime};

use crate::{
    error::AppError,
    ports::{
        Clock, CreateUserOutcome, EmailDelivery, EmailStatus, NewSession, NewUser, PasswordHasher,
        RateLimitDecision, RateLimitRule, RateLimiter, SessionRepository, TokenService,
        UserRepository, VerifyEmailOutcome,
    },
    ratelimit::{RateLimitConfig, login_ip_key, login_user_key, register_ip_key},
};

/// How long an emailed verification link works.
const EMAIL_VERIFICATION_TTL: Duration = Duration::hours(24);
/// Verification emails a user may request per hour.
const EMAIL_VERIFICATION_SENDS_PER_HOUR: u32 = 3;

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

/// Idle timeout and token rotation policy for sessions.
///
/// The absolute lifetime is `AuthConfig::session_ttl`. A session also ends
/// after `idle_timeout` without activity, and its cookie token is replaced
/// every `rotation_interval`; the previous token keeps working for
/// `rotation_grace` so parallel in-flight requests are not signed out.
#[derive(Debug, Clone, Copy)]
pub struct SessionLifecycle {
    pub idle_timeout: Duration,
    pub rotation_interval: Duration,
    pub rotation_grace: Duration,
    /// Activity is written at most this often, to avoid a write per request.
    pub touch_interval: Duration,
}

impl Default for SessionLifecycle {
    fn default() -> Self {
        Self {
            idle_timeout: Duration::days(7),
            rotation_interval: Duration::hours(24),
            rotation_grace: Duration::seconds(60),
            touch_interval: Duration::minutes(5),
        }
    }
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

/// A request's principal, plus a replacement cookie token when this request
/// rotated the session token. The token is secret and redacted from `Debug`.
pub struct AuthenticatedRequest {
    pub context: AuthContext,
    pub rotated_token: Option<String>,
}

impl std::fmt::Debug for AuthenticatedRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthenticatedRequest")
            .field("context", &self.context)
            .field(
                "rotated_token",
                &self.rotated_token.as_ref().map(|_| "<redacted>"),
            )
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
    instance_admin_ids: HashSet<UserId>,
    email_delivery: Option<Arc<dyn EmailDelivery>>,
    public_url: Option<String>,
    lifecycle: SessionLifecycle,
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
            instance_admin_ids: HashSet::new(),
            email_delivery: None,
            public_url: None,
            lifecycle: SessionLifecycle::default(),
            config,
        }
    }

    pub fn with_session_lifecycle(mut self, lifecycle: SessionLifecycle) -> Self {
        self.lifecycle = lifecycle;
        self
    }

    pub fn with_instance_admins(mut self, admin_ids: impl IntoIterator<Item = UserId>) -> Self {
        self.instance_admin_ids = admin_ids.into_iter().collect();
        self
    }

    fn require_instance_admin(&self, actor: UserId) -> Result<(), AppError> {
        self.instance_admin_ids
            .contains(&actor)
            .then_some(())
            .ok_or(AppError::InstanceAdminRequired)
    }

    pub async fn list_instance_users(
        &self,
        actor: UserId,
    ) -> Result<Vec<crate::ports::AdminUser>, AppError> {
        self.require_instance_admin(actor)?;
        let mut users = self.users.list_admin_users(100).await?;
        for user in &mut users {
            user.is_instance_admin = self.instance_admin_ids.contains(&user.user.id);
        }
        Ok(users)
    }

    pub async fn set_user_suspended(
        &self,
        actor: UserId,
        target: UserId,
        suspended: bool,
    ) -> Result<crate::ports::AdminUser, AppError> {
        self.require_instance_admin(actor)?;
        if actor == target {
            return Err(AppError::CannotSuspendSelf);
        }
        if self.instance_admin_ids.contains(&target) {
            return Err(AppError::CannotModifyInstanceAdmin);
        }
        let mut user = self
            .users
            .set_user_suspended(actor, target, suspended, self.clock.now())
            .await?;
        user.is_instance_admin = false;
        Ok(user)
    }

    pub fn with_email_delivery(
        mut self,
        delivery: Arc<dyn EmailDelivery>,
        public_url: String,
    ) -> Self {
        self.email_delivery = Some(delivery);
        self.public_url = Some(public_url.trim_end_matches('/').to_owned());
        self
    }

    fn email_delivery(&self) -> Result<(&dyn EmailDelivery, &str), AppError> {
        let delivery = self
            .email_delivery
            .as_deref()
            .ok_or(AppError::EmailUnavailable)?;
        let public_url = self
            .public_url
            .as_deref()
            .ok_or(AppError::EmailUnavailable)?;
        Ok((delivery, public_url))
    }

    pub async fn request_password_reset(
        &self,
        client_ip: IpAddr,
        raw_email: &str,
    ) -> Result<(), AppError> {
        let (delivery, public_url) = self.email_delivery()?;
        let now = self.clock.now();
        let ip_key = login_ip_key(client_ip).replace("login:", "recovery:");
        let ip_rule = self.config.rate_limits.login_per_ip;
        let email_rule = self.config.rate_limits.login_failures_per_user;
        self.enforce(&ip_key, ip_rule, now).await?;
        self.limiter.record(&ip_key, ip_rule, now).await?;

        let email = EmailAddress::parse(raw_email)?;
        let email_digest = self.tokens.digest(email.as_str());
        let email_key = format!("recovery:email:{:02x?}", email_digest.0);
        self.enforce(&email_key, email_rule, now).await?;
        self.limiter.record(&email_key, email_rule, now).await?;

        let token = self.tokens.generate();
        let raw = token.raw;
        let digest = token.digest;
        let expires_at = now + Duration::hours(1);
        if self
            .users
            .issue_password_reset_token(&email, digest, now, expires_at)
            .await?
        {
            let link = format!("{public_url}/reset-password?token={raw}");
            if delivery
                .send(
                    &email,
                    "Reset your RustChat password",
                    &format!("Reset your password by opening this link:\n\n{link}\n\nThis link expires in one hour. If you did not request it, you can ignore this message."),
                )
                .await
                .is_err()
            {
                tracing::warn!("password reset email delivery failed");
            }
        }
        Ok(())
    }

    /// The caller's recovery address and whether it is verified.
    pub async fn email_status(&self, user_id: UserId) -> Result<EmailStatus, AppError> {
        self.users.email_status(user_id).await
    }

    /// Set or clear the recovery address after re-checking the password. A new
    /// address starts unverified; a verification link is sent when email
    /// delivery is configured.
    pub async fn set_email(
        &self,
        auth: &AuthContext,
        client_ip: IpAddr,
        current_password: String,
        email: Option<&str>,
    ) -> Result<EmailStatus, AppError> {
        let email = email
            .filter(|value| !value.trim().is_empty())
            .map(EmailAddress::parse)
            .transpose()?;
        self.verify_current_password(auth, client_ip, current_password)
            .await?;
        self.users.set_email(auth.user.id, email.as_ref()).await?;
        if let (Some(email), true) = (email, self.email_delivery.is_some())
            && let Err(error) = self.send_email_verification(auth.user.id, &email).await
        {
            tracing::warn!(%error, "verification email could not be sent");
        }
        self.users.email_status(auth.user.id).await
    }

    /// Send a fresh verification link to the caller's unverified address.
    /// Does nothing when there is no address or it is already verified.
    pub async fn resend_email_verification(&self, auth: &AuthContext) -> Result<(), AppError> {
        self.email_delivery()?;
        let now = self.clock.now();
        let rule = RateLimitRule {
            max: EMAIL_VERIFICATION_SENDS_PER_HOUR,
            window: Duration::hours(1),
        };
        let key = format!("email-verify:user:{}", auth.user.id.as_uuid());
        self.enforce(&key, rule, now).await?;
        self.limiter.record(&key, rule, now).await?;
        let status = self.users.email_status(auth.user.id).await?;
        match status.address {
            Some(address) if !status.verified => {
                self.send_email_verification(auth.user.id, &address).await
            }
            _ => Ok(()),
        }
    }

    /// Confirm an address from the emailed link. The link is single-use and
    /// works from any browser, so it does not require a session.
    pub async fn verify_email(&self, client_ip: IpAddr, raw_token: &str) -> Result<(), AppError> {
        if raw_token.len() > 128 || raw_token.is_empty() {
            return Err(AppError::InvalidAccountToken);
        }
        let now = self.clock.now();
        let rule = self.config.rate_limits.login_per_ip;
        let key = login_ip_key(client_ip).replace("login:", "verify:");
        self.enforce(&key, rule, now).await?;
        self.limiter.record(&key, rule, now).await?;
        let digest = self.tokens.digest(raw_token);
        match self.users.verify_email_with_token(digest, now).await? {
            VerifyEmailOutcome::Verified => Ok(()),
            VerifyEmailOutcome::Invalid => Err(AppError::InvalidAccountToken),
            VerifyEmailOutcome::AddressInUse => Err(AppError::EmailAddressInUse),
        }
    }

    async fn send_email_verification(
        &self,
        user_id: UserId,
        email: &EmailAddress,
    ) -> Result<(), AppError> {
        let (delivery, public_url) = self.email_delivery()?;
        let now = self.clock.now();
        let token = self.tokens.generate();
        if !self
            .users
            .issue_email_verification_token(
                user_id,
                email,
                token.digest,
                now,
                now + EMAIL_VERIFICATION_TTL,
            )
            .await?
        {
            return Ok(());
        }
        let link = format!("{public_url}/verify-email?token={}", token.raw);
        delivery
            .send(
                email,
                "Confirm your RustChat email address",
                &format!("Confirm this address for your RustChat account by opening this link:\n\n{link}\n\nThis link expires in 24 hours. If you did not expect this message, you can ignore it."),
            )
            .await
    }

    pub async fn complete_password_reset(
        &self,
        raw_token: &str,
        new_password: String,
    ) -> Result<(), AppError> {
        if raw_token.len() > 128 || raw_token.is_empty() {
            return Err(AppError::InvalidAccountToken);
        }
        let password = PlainPassword::parse_new(new_password)?;
        let hash = self.hasher.hash(password).await?;
        let digest = self.tokens.digest(raw_token);
        if self
            .users
            .reset_password_with_token(digest, hash, self.clock.now())
            .await?
        {
            Ok(())
        } else {
            Err(AppError::InvalidAccountToken)
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
        self.register_with_email(client_ip, username, None, password)
            .await
    }

    pub async fn register_with_email(
        &self,
        client_ip: IpAddr,
        username: &str,
        email: Option<&str>,
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
        let email = email.map(EmailAddress::parse).transpose()?;
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
                email: email.clone(),
                password_hash,
                created_at: user.created_at,
            })
            .await?;
        match outcome {
            CreateUserOutcome::Created => {
                // Best effort: the account exists either way, and the owner can
                // ask for another email from their settings.
                if let (Some(email), true) = (email, self.email_delivery.is_some())
                    && let Err(error) = self.send_email_verification(user.id, &email).await
                {
                    tracing::warn!(%error, "verification email could not be sent at registration");
                }
                Ok(user)
            }
            CreateUserOutcome::UsernameTaken => Err(AppError::UsernameTaken),
            CreateUserOutcome::EmailTaken => Err(AppError::EmailTaken),
        }
    }

    pub async fn profile(&self, user_id: UserId) -> Result<UserProfile, AppError> {
        self.users.get_profile(user_id).await
    }

    pub async fn user_profile(
        &self,
        caller: UserId,
        username: &str,
    ) -> Result<UserProfile, AppError> {
        let username = Username::parse(username)?;
        self.users
            .find_profile_for_caller(caller, &username)
            .await?
            .ok_or(AppError::NotFound)
    }

    pub async fn privacy(&self, user_id: UserId) -> Result<ProfilePrivacy, AppError> {
        self.users.get_privacy(user_id).await
    }

    pub async fn update_privacy(
        &self,
        user_id: UserId,
        privacy: ProfilePrivacy,
    ) -> Result<ProfilePrivacy, AppError> {
        self.users.update_privacy(user_id, privacy).await
    }

    pub async fn update_username(&self, user_id: UserId, username: &str) -> Result<User, AppError> {
        let username = Username::parse(username)?;
        self.users.update_username(user_id, &username).await
    }

    pub async fn update_profile(
        &self,
        user_id: UserId,
        display_name: &str,
        bio: &str,
    ) -> Result<UserProfile, AppError> {
        if display_name.chars().count() > 64 || bio.chars().count() > 280 {
            return Err(AppError::InvalidProfile);
        }
        self.users
            .update_profile(user_id, display_name.trim(), bio.trim(), self.clock.now())
            .await
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

    /// Resolve a session token to its principal, or `Unauthenticated`, without
    /// recording activity or rotating the token. Long-lived connections use
    /// this to re-check that the session is still valid.
    pub async fn authenticate(&self, raw_token: &str) -> Result<AuthContext, AppError> {
        let digest = self.tokens.digest(raw_token);
        let now = self.clock.now();
        Ok(self.find_live_session(&digest, now).await?.0)
    }

    /// Resolve a session token for an HTTP request: enforces the idle timeout,
    /// records activity and rotates the token when it is due.
    pub async fn authenticate_request(
        &self,
        raw_token: &str,
    ) -> Result<AuthenticatedRequest, AppError> {
        let digest = self.tokens.digest(raw_token);
        let now = self.clock.now();
        let (context, active) = self.find_live_session(&digest, now).await?;
        let life = self.lifecycle;
        let mut rotated_token = None;
        if !active.via_previous_token && now - active.token_rotated_at >= life.rotation_interval {
            let token = self.tokens.generate();
            if self
                .sessions
                .rotate_token(
                    context.session_id,
                    &digest,
                    &token.digest,
                    now,
                    now + life.rotation_grace,
                )
                .await?
            {
                rotated_token = Some(token.raw);
            }
        } else if now - active.last_seen_at >= life.touch_interval {
            self.sessions.touch(context.session_id, now).await?;
        }
        Ok(AuthenticatedRequest {
            context,
            rotated_token,
        })
    }

    async fn find_live_session(
        &self,
        digest: &crate::ports::TokenDigest,
        now: OffsetDateTime,
    ) -> Result<(AuthContext, crate::ports::ActiveSession), AppError> {
        match self.sessions.find_active(digest, now).await? {
            Some(active)
                if !active.session.is_expired_at(now)
                    && now - active.last_seen_at < self.lifecycle.idle_timeout =>
            {
                let context = AuthContext {
                    user: active.user.clone(),
                    session_id: active.session.id,
                };
                Ok((context, active))
            }
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

    /// Change credentials after proving knowledge of the current password.
    /// The current browser remains signed in; every other session is revoked
    /// in the same database transaction as the password update.
    pub async fn change_password(
        &self,
        auth: &AuthContext,
        client_ip: IpAddr,
        current_password: String,
        new_password: String,
    ) -> Result<(), AppError> {
        let now = self.clock.now();
        let ip_key = login_ip_key(client_ip);
        let user_key = login_user_key(auth.user.username.as_str());
        self.enforce(&ip_key, self.config.rate_limits.login_per_ip, now)
            .await?;
        self.enforce(
            &user_key,
            self.config.rate_limits.login_failures_per_user,
            now,
        )
        .await?;
        self.limiter
            .record(&ip_key, self.config.rate_limits.login_per_ip, now)
            .await?;

        let Some(current_password) = PlainPassword::from_login_input(current_password) else {
            self.limiter
                .record(
                    &user_key,
                    self.config.rate_limits.login_failures_per_user,
                    now,
                )
                .await?;
            return Err(AppError::InvalidCredentials);
        };
        let credentials = self
            .users
            .find_credentials(&auth.user.username)
            .await?
            .ok_or(AppError::Unauthenticated)?;
        if !self
            .hasher
            .verify(current_password, credentials.password_hash)
            .await?
        {
            self.limiter
                .record(
                    &user_key,
                    self.config.rate_limits.login_failures_per_user,
                    now,
                )
                .await?;
            return Err(AppError::InvalidCredentials);
        }

        let new_password = PlainPassword::parse_new(new_password)?;
        let password_hash = self.hasher.hash(new_password).await?;
        self.sessions
            .update_password_and_revoke_other_sessions(
                auth.user.id,
                auth.session_id,
                password_hash,
                self.clock.now(),
            )
            .await?;
        self.limiter.reset(&user_key).await?;
        Ok(())
    }

    pub async fn deactivate_account(
        &self,
        auth: &AuthContext,
        client_ip: IpAddr,
        current_password: String,
    ) -> Result<(), AppError> {
        self.verify_current_password(auth, client_ip, current_password)
            .await?;
        self.users
            .deactivate_account(auth.user.id, self.clock.now())
            .await
    }

    pub async fn delete_account(
        &self,
        auth: &AuthContext,
        client_ip: IpAddr,
        current_password: String,
    ) -> Result<(), AppError> {
        self.verify_current_password(auth, client_ip, current_password)
            .await?;
        let opaque_secret = format!("deleted-{}", self.tokens.generate().raw);
        let unusable_password_hash = self
            .hasher
            .hash(PlainPassword::parse_new(opaque_secret)?)
            .await?;
        let compact_id = auth.user.id.as_uuid().simple().to_string();
        let tombstone_username = Username::parse(&format!("deleted_{}", &compact_id[..24]))?;
        self.users
            .delete_account(
                auth.user.id,
                &tombstone_username,
                &unusable_password_hash,
                self.clock.now(),
            )
            .await
    }

    async fn verify_current_password(
        &self,
        auth: &AuthContext,
        client_ip: IpAddr,
        raw: String,
    ) -> Result<(), AppError> {
        let now = self.clock.now();
        let ip_key = login_ip_key(client_ip);
        let user_key = login_user_key(auth.user.username.as_str());
        self.enforce(&ip_key, self.config.rate_limits.login_per_ip, now)
            .await?;
        self.enforce(
            &user_key,
            self.config.rate_limits.login_failures_per_user,
            now,
        )
        .await?;
        self.limiter
            .record(&ip_key, self.config.rate_limits.login_per_ip, now)
            .await?;
        let Some(password) = PlainPassword::from_login_input(raw) else {
            self.limiter
                .record(
                    &user_key,
                    self.config.rate_limits.login_failures_per_user,
                    now,
                )
                .await?;
            return Err(AppError::InvalidCredentials);
        };
        let credentials = self
            .users
            .find_credentials(&auth.user.username)
            .await?
            .ok_or(AppError::Unauthenticated)?;
        if self
            .hasher
            .verify(password, credentials.password_hash)
            .await?
        {
            self.limiter.reset(&user_key).await?;
            Ok(())
        } else {
            self.limiter
                .record(
                    &user_key,
                    self.config.rate_limits.login_failures_per_user,
                    now,
                )
                .await?;
            Err(AppError::InvalidCredentials)
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

    async fn logged_in(h: &Harness) -> LoginResult {
        h.auth.register(IP, "alice", PASSWORD.into()).await.unwrap();
        h.auth.login(IP, "alice", PASSWORD.into()).await.unwrap()
    }

    #[tokio::test]
    async fn session_ends_after_the_idle_timeout() {
        let h = Harness::new(RegistrationPolicy::Open);
        let idle = SessionLifecycle::default().idle_timeout;
        let login = logged_in(&h).await;

        h.clock.advance(idle - Duration::seconds(1));
        assert!(h.auth.authenticate(&login.token).await.is_ok());
        h.clock.advance(Duration::seconds(1));
        assert!(matches!(
            h.auth.authenticate(&login.token).await,
            Err(AppError::Unauthenticated)
        ));
        assert!(matches!(
            h.auth.authenticate_request(&login.token).await,
            Err(AppError::Unauthenticated)
        ));
    }

    #[tokio::test]
    async fn requests_record_activity_so_an_active_session_is_not_idle() {
        let h = Harness::new(RegistrationPolicy::Open).with_lifecycle(SessionLifecycle {
            // Rotation is exercised separately; keep this token stable.
            rotation_interval: Duration::days(365),
            ..SessionLifecycle::default()
        });
        let idle = SessionLifecycle::default().idle_timeout;
        let login = logged_in(&h).await;

        for _ in 0..3 {
            h.clock.advance(idle - Duration::hours(1));
            h.auth.authenticate_request(&login.token).await.unwrap();
        }
        // Three idle-timeout-sized gaps have passed, but each one was bridged by a request.
        assert!(h.auth.authenticate(&login.token).await.is_ok());
        // Without further activity the session still ends.
        h.clock.advance(idle);
        assert!(h.auth.authenticate(&login.token).await.is_err());
    }

    #[tokio::test]
    async fn token_rotates_when_due_and_the_old_one_works_only_briefly() {
        let h = Harness::new(RegistrationPolicy::Open);
        let life = SessionLifecycle::default();
        let login = logged_in(&h).await;

        // Not due yet: no replacement token.
        h.clock
            .advance(life.rotation_interval - Duration::seconds(1));
        let early = h.auth.authenticate_request(&login.token).await.unwrap();
        assert!(early.rotated_token.is_none());

        h.clock.advance(Duration::seconds(1));
        let rotated = h.auth.authenticate_request(&login.token).await.unwrap();
        let new_token = rotated.rotated_token.expect("token due for rotation");
        assert_ne!(new_token, login.token);
        assert_eq!(rotated.context.session_id, login.session.id);

        // The replacement works and is not rotated again immediately.
        let again = h.auth.authenticate_request(&new_token).await.unwrap();
        assert!(again.rotated_token.is_none());
        assert_eq!(again.context.session_id, login.session.id);

        // The previous token survives only for the grace period, and never rotates again.
        h.clock.advance(life.rotation_grace - Duration::seconds(1));
        let in_flight = h.auth.authenticate_request(&login.token).await.unwrap();
        assert!(in_flight.rotated_token.is_none());
        h.clock.advance(Duration::seconds(1));
        assert!(matches!(
            h.auth.authenticate(&login.token).await,
            Err(AppError::Unauthenticated)
        ));
        assert!(h.auth.authenticate(&new_token).await.is_ok());
    }

    #[tokio::test]
    async fn passive_authentication_neither_rotates_nor_records_activity() {
        let h = Harness::new(RegistrationPolicy::Open);
        let life = SessionLifecycle::default();
        let login = logged_in(&h).await;
        h.clock.advance(life.rotation_interval + Duration::hours(1));
        h.auth.authenticate(&login.token).await.unwrap();
        // The request path still sees the rotation as due, because the passive call did nothing.
        let request = h.auth.authenticate_request(&login.token).await.unwrap();
        assert!(request.rotated_token.is_some());
    }

    async fn registered_with_email(h: &Harness, email: &str) -> AuthContext {
        h.auth
            .register_with_email(IP, "alice", Some(email), PASSWORD.into())
            .await
            .unwrap();
        let login = h.auth.login(IP, "alice", PASSWORD.into()).await.unwrap();
        h.auth.authenticate(&login.token).await.unwrap()
    }

    #[tokio::test]
    async fn registration_with_an_email_sends_a_verification_link_that_works_once() {
        let h = Harness::new(RegistrationPolicy::Open);
        let auth = registered_with_email(&h, "Alice@Example.test").await;

        let status = h.auth.email_status(auth.user.id).await.unwrap();
        assert_eq!(status.address.unwrap().as_str(), "alice@example.test");
        assert!(!status.verified);
        let token = h.emails.last_token_for("alice@example.test").unwrap();

        h.auth.verify_email(IP, &token).await.unwrap();
        assert!(h.auth.email_status(auth.user.id).await.unwrap().verified);
        assert!(matches!(
            h.auth.verify_email(IP, &token).await,
            Err(AppError::InvalidAccountToken)
        ));
    }

    #[tokio::test]
    async fn a_verification_link_expires_after_a_day() {
        let h = Harness::new(RegistrationPolicy::Open);
        let auth = registered_with_email(&h, "alice@example.test").await;
        let token = h.emails.last_token_for("alice@example.test").unwrap();

        h.clock.advance(Duration::hours(24));
        assert!(matches!(
            h.auth.verify_email(IP, &token).await,
            Err(AppError::InvalidAccountToken)
        ));
        assert!(!h.auth.email_status(auth.user.id).await.unwrap().verified);
    }

    #[tokio::test]
    async fn changing_the_email_needs_the_password_and_resets_verification() {
        let h = Harness::new(RegistrationPolicy::Open);
        let auth = registered_with_email(&h, "old@example.test").await;
        let old = h.emails.last_token_for("old@example.test").unwrap();
        h.auth.verify_email(IP, &old).await.unwrap();

        assert!(matches!(
            h.auth
                .set_email(
                    &auth,
                    IP,
                    "wrong password!!".into(),
                    Some("new@example.test")
                )
                .await,
            Err(AppError::InvalidCredentials)
        ));
        assert!(h.auth.email_status(auth.user.id).await.unwrap().verified);

        let status = h
            .auth
            .set_email(&auth, IP, PASSWORD.into(), Some("new@example.test"))
            .await
            .unwrap();
        assert_eq!(status.address.unwrap().as_str(), "new@example.test");
        assert!(!status.verified);
        assert!(h.emails.last_token_for("new@example.test").is_some());
    }

    #[tokio::test]
    async fn resend_is_limited_to_three_emails_per_hour() {
        let h = Harness::new(RegistrationPolicy::Open);
        let auth = registered_with_email(&h, "alice@example.test").await;
        let sent_at_registration = h.emails.sent().len();

        for _ in 0..3 {
            h.auth.resend_email_verification(&auth).await.unwrap();
        }
        assert_eq!(h.emails.sent().len(), sent_at_registration + 3);
        assert!(matches!(
            h.auth.resend_email_verification(&auth).await,
            Err(AppError::RateLimited { .. })
        ));
    }

    #[tokio::test]
    async fn resend_sends_nothing_once_the_address_is_verified() {
        let h = Harness::new(RegistrationPolicy::Open);
        let auth = registered_with_email(&h, "alice@example.test").await;
        let token = h.emails.last_token_for("alice@example.test").unwrap();
        h.auth.verify_email(IP, &token).await.unwrap();

        let before = h.emails.sent().len();
        h.auth.resend_email_verification(&auth).await.unwrap();
        assert_eq!(h.emails.sent().len(), before);
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
        // This test is about the absolute lifetime, so keep the idle timeout out of the way.
        let h = Harness::new(RegistrationPolicy::Open).with_lifecycle(SessionLifecycle {
            idle_timeout: Duration::days(365),
            ..SessionLifecycle::default()
        });
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
    async fn deactivation_revokes_sessions_and_a_new_login_reactivates_the_account() {
        let h = Harness::new(RegistrationPolicy::Open);
        h.auth.register(IP, "alice", PASSWORD.into()).await.unwrap();
        let first = h.auth.login(IP, "alice", PASSWORD.into()).await.unwrap();
        let second = h.auth.login(IP, "alice", PASSWORD.into()).await.unwrap();
        let ctx = h.auth.authenticate(&first.token).await.unwrap();
        h.auth
            .deactivate_account(&ctx, IP, PASSWORD.into())
            .await
            .unwrap();
        assert!(h.auth.authenticate(&first.token).await.is_err());
        assert!(h.auth.authenticate(&second.token).await.is_err());
        let reactivated = h.auth.login(IP, "alice", PASSWORD.into()).await.unwrap();
        assert!(h.auth.authenticate(&reactivated.token).await.is_ok());
        assert!(h.auth.authenticate(&first.token).await.is_err());
    }

    #[tokio::test]
    async fn deletion_revokes_sessions_and_prevents_future_login() {
        let h = Harness::new(RegistrationPolicy::Open);
        h.auth.register(IP, "alice", PASSWORD.into()).await.unwrap();
        let login = h.auth.login(IP, "alice", PASSWORD.into()).await.unwrap();
        let ctx = h.auth.authenticate(&login.token).await.unwrap();
        h.auth
            .delete_account(&ctx, IP, PASSWORD.into())
            .await
            .unwrap();
        assert!(h.auth.authenticate(&login.token).await.is_err());
        assert!(matches!(
            h.auth.login(IP, "alice", PASSWORD.into()).await,
            Err(AppError::InvalidCredentials)
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
