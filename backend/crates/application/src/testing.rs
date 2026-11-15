//! In-memory fakes for application unit tests.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use rustchat_domain::identity::{
    PlainPassword, ProfilePrivacy, ProfileVisibility, Session, SessionId, User, UserId, Username,
};
use time::{Duration, OffsetDateTime};

use crate::{
    auth::{AuthConfig, AuthService, RegistrationPolicy},
    error::AppError,
    ports::*,
    ratelimit::RateLimitConfig,
};

#[derive(Default)]
struct Store {
    users: Vec<(User, String)>,
    sessions: Vec<(Session, TokenDigest, bool)>,
    privacy: HashMap<UserId, ProfilePrivacy>,
    deactivated: HashSet<UserId>,
    suspended: HashMap<UserId, OffsetDateTime>,
    deleted: HashSet<UserId>,
}

#[derive(Clone, Default)]
pub struct FakeStore(Arc<Mutex<Store>>);

#[async_trait]
impl UserRepository for FakeStore {
    async fn create(&self, user: NewUser) -> Result<CreateUserOutcome, AppError> {
        let mut store = self.0.lock().unwrap();
        if store.users.iter().any(|(u, _)| u.username == user.username) {
            return Ok(CreateUserOutcome::UsernameTaken);
        }
        let record = User {
            id: user.id,
            username: user.username,
            created_at: user.created_at,
        };
        store.users.push((record, user.password_hash));
        Ok(CreateUserOutcome::Created)
    }

    async fn find_credentials(
        &self,
        username: &Username,
    ) -> Result<Option<UserCredentials>, AppError> {
        let store = self.0.lock().unwrap();
        Ok(store
            .users
            .iter()
            .find(|(u, _)| &u.username == username && !store.deleted.contains(&u.id))
            .map(|(u, hash)| UserCredentials {
                user: u.clone(),
                password_hash: hash.clone(),
            }))
    }

    async fn find_discoverable(
        &self,
        _caller: UserId,
        username: &Username,
    ) -> Result<Option<User>, AppError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .users
            .iter()
            .find(|(u, _)| &u.username == username)
            .map(|(u, _)| u.clone()))
    }

    async fn find_profile_for_caller(
        &self,
        caller: UserId,
        username: &Username,
    ) -> Result<Option<rustchat_domain::identity::UserProfile>, AppError> {
        let store = self.0.lock().unwrap();
        let Some((user, _)) = store.users.iter().find(|(u, _)| &u.username == username) else {
            return Ok(None);
        };
        let privacy = store.privacy.get(&user.id).copied().unwrap_or_default();
        if caller != user.id
            && !(privacy.discoverable && privacy.visibility == ProfileVisibility::Everyone)
        {
            return Ok(None);
        }
        Ok(Some(rustchat_domain::identity::UserProfile {
            user_id: user.id,
            username: user.username.clone(),
            display_name: user.username.to_string(),
            bio: String::new(),
            updated_at: user.created_at,
        }))
    }

    async fn update_username(
        &self,
        user_id: UserId,
        username: &Username,
    ) -> Result<User, AppError> {
        let mut store = self.0.lock().unwrap();
        if store
            .users
            .iter()
            .any(|(user, _)| user.username == *username && user.id != user_id)
        {
            return Err(AppError::UsernameTaken);
        }
        let (user, _) = store
            .users
            .iter_mut()
            .find(|(user, _)| user.id == user_id)
            .ok_or(AppError::NotFound)?;
        user.username = username.clone();
        Ok(user.clone())
    }

    async fn get_profile(
        &self,
        user_id: UserId,
    ) -> Result<rustchat_domain::identity::UserProfile, AppError> {
        let store = self.0.lock().unwrap();
        let (user, _) = store
            .users
            .iter()
            .find(|(user, _)| user.id == user_id)
            .ok_or(AppError::NotFound)?;
        Ok(rustchat_domain::identity::UserProfile {
            user_id,
            username: user.username.clone(),
            display_name: user.username.to_string(),
            bio: String::new(),
            updated_at: user.created_at,
        })
    }

    async fn update_profile(
        &self,
        user_id: UserId,
        display_name: &str,
        bio: &str,
        now: OffsetDateTime,
    ) -> Result<rustchat_domain::identity::UserProfile, AppError> {
        let store = self.0.lock().unwrap();
        let (user, _) = store
            .users
            .iter()
            .find(|(user, _)| user.id == user_id)
            .ok_or(AppError::NotFound)?;
        Ok(rustchat_domain::identity::UserProfile {
            user_id,
            username: user.username.clone(),
            display_name: display_name.to_owned(),
            bio: bio.to_owned(),
            updated_at: now,
        })
    }

    async fn get_privacy(&self, user_id: UserId) -> Result<ProfilePrivacy, AppError> {
        if !self
            .0
            .lock()
            .unwrap()
            .users
            .iter()
            .any(|(user, _)| user.id == user_id)
        {
            return Err(AppError::NotFound);
        }
        Ok(self
            .0
            .lock()
            .unwrap()
            .privacy
            .get(&user_id)
            .copied()
            .unwrap_or_default())
    }

    async fn update_privacy(
        &self,
        user_id: UserId,
        privacy: ProfilePrivacy,
    ) -> Result<ProfilePrivacy, AppError> {
        let mut store = self.0.lock().unwrap();
        if !store.users.iter().any(|(user, _)| user.id == user_id) {
            return Err(AppError::NotFound);
        }
        store.privacy.insert(user_id, privacy);
        Ok(privacy)
    }

    async fn deactivate_account(
        &self,
        user_id: UserId,
        _now: OffsetDateTime,
    ) -> Result<(), AppError> {
        let mut store = self.0.lock().unwrap();
        if !store.users.iter().any(|(user, _)| user.id == user_id) {
            return Err(AppError::NotFound);
        }
        store.deactivated.insert(user_id);
        for (session, _, revoked) in &mut store.sessions {
            if session.user_id == user_id {
                *revoked = true;
            }
        }
        Ok(())
    }

    async fn delete_account(
        &self,
        user_id: UserId,
        tombstone_username: &Username,
        hash: &str,
        _now: OffsetDateTime,
    ) -> Result<(), AppError> {
        let mut store = self.0.lock().unwrap();
        let (user, password_hash) = store
            .users
            .iter_mut()
            .find(|(user, _)| user.id == user_id)
            .ok_or(AppError::NotFound)?;
        user.username = tombstone_username.clone();
        *password_hash = hash.to_owned();
        store.deleted.insert(user_id);
        store.deactivated.insert(user_id);
        for (session, _, revoked) in &mut store.sessions {
            if session.user_id == user_id {
                *revoked = true;
            }
        }
        Ok(())
    }

    async fn issue_password_reset_token(
        &self,
        _email: &rustchat_domain::identity::EmailAddress,
        _digest: TokenDigest,
        _now: OffsetDateTime,
        _expires_at: OffsetDateTime,
    ) -> Result<bool, AppError> {
        Ok(false)
    }

    async fn reset_password_with_token(
        &self,
        _digest: TokenDigest,
        _password_hash: String,
        _now: OffsetDateTime,
    ) -> Result<bool, AppError> {
        Ok(false)
    }

    async fn list_admin_users(&self, limit: i64) -> Result<Vec<AdminUser>, AppError> {
        let store = self.0.lock().unwrap();
        Ok(store
            .users
            .iter()
            .take(limit as usize)
            .map(|(user, _)| AdminUser {
                user: user.clone(),
                is_instance_admin: false,
                deactivated_at: store
                    .deactivated
                    .contains(&user.id)
                    .then_some(OffsetDateTime::UNIX_EPOCH),
                suspended_at: store.suspended.get(&user.id).copied(),
                deleted_at: store
                    .deleted
                    .contains(&user.id)
                    .then_some(OffsetDateTime::UNIX_EPOCH),
            })
            .collect())
    }

    async fn set_user_suspended(
        &self,
        _actor: UserId,
        target: UserId,
        suspended: bool,
        now: OffsetDateTime,
    ) -> Result<AdminUser, AppError> {
        let mut store = self.0.lock().unwrap();
        let user = store
            .users
            .iter()
            .find(|(user, _)| user.id == target)
            .map(|(user, _)| user.clone())
            .ok_or(AppError::NotFound)?;
        if store.deleted.contains(&target) {
            return Err(AppError::NotFound);
        }
        if suspended {
            store.suspended.insert(target, now);
        } else {
            store.suspended.remove(&target);
        }
        for (session, _, revoked) in &mut store.sessions {
            if session.user_id == target && suspended {
                *revoked = true;
            }
        }
        Ok(AdminUser {
            user,
            is_instance_admin: false,
            deactivated_at: store
                .deactivated
                .contains(&target)
                .then_some(OffsetDateTime::UNIX_EPOCH),
            suspended_at: store.suspended.get(&target).copied(),
            deleted_at: store
                .deleted
                .contains(&target)
                .then_some(OffsetDateTime::UNIX_EPOCH),
        })
    }
}

#[async_trait]
impl SessionRepository for FakeStore {
    async fn insert(&self, new: NewSession) -> Result<(), AppError> {
        let mut store = self.0.lock().unwrap();
        if store.deleted.contains(&new.session.user_id) {
            return Err(AppError::Unauthenticated);
        }
        store.deactivated.remove(&new.session.user_id);
        store.sessions.push((new.session, new.token_digest, false));
        Ok(())
    }

    async fn find_active(
        &self,
        digest: &TokenDigest,
        now: OffsetDateTime,
    ) -> Result<Option<ActiveSession>, AppError> {
        let store = self.0.lock().unwrap();
        let found = store.sessions.iter().find(|(s, d, revoked)| {
            d == digest
                && !revoked
                && !s.is_expired_at(now)
                && !store.deactivated.contains(&s.user_id)
                && !store.suspended.contains_key(&s.user_id)
                && !store.deleted.contains(&s.user_id)
        });
        Ok(found.and_then(|(s, _, _)| {
            store
                .users
                .iter()
                .find(|(u, _)| u.id == s.user_id)
                .map(|(u, _)| ActiveSession {
                    session: s.clone(),
                    user: u.clone(),
                })
        }))
    }

    async fn list_active(
        &self,
        user_id: UserId,
        now: OffsetDateTime,
    ) -> Result<Vec<Session>, AppError> {
        let store = self.0.lock().unwrap();
        Ok(store
            .sessions
            .iter()
            .filter(|(s, _, revoked)| s.user_id == user_id && !revoked && !s.is_expired_at(now))
            .map(|(s, _, _)| s.clone())
            .collect())
    }

    async fn revoke(
        &self,
        user_id: UserId,
        session_id: SessionId,
        _now: OffsetDateTime,
    ) -> Result<bool, AppError> {
        let mut store = self.0.lock().unwrap();
        match store
            .sessions
            .iter_mut()
            .find(|(s, _, revoked)| s.id == session_id && s.user_id == user_id && !*revoked)
        {
            Some(entry) => {
                entry.2 = true;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    async fn update_password_and_revoke_other_sessions(
        &self,
        user_id: UserId,
        current_session_id: SessionId,
        password_hash: String,
        _now: OffsetDateTime,
    ) -> Result<(), AppError> {
        let mut store = self.0.lock().unwrap();
        let (_, hash) = store
            .users
            .iter_mut()
            .find(|(user, _)| user.id == user_id)
            .ok_or(AppError::NotFound)?;
        *hash = password_hash;
        for (session, _, revoked) in &mut store.sessions {
            if session.user_id == user_id && session.id != current_session_id {
                *revoked = true;
            }
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct FakeHasher {
    dummy: Mutex<usize>,
    real: Mutex<usize>,
}

impl FakeHasher {
    pub fn dummy_verifications(&self) -> usize {
        *self.dummy.lock().unwrap()
    }

    /// Total password work done: hashes, verifications and dummy verifications.
    pub fn password_work(&self) -> usize {
        *self.dummy.lock().unwrap() + *self.real.lock().unwrap()
    }
}

#[async_trait]
impl PasswordHasher for FakeHasher {
    async fn hash(&self, password: PlainPassword) -> Result<String, AppError> {
        *self.real.lock().unwrap() += 1;
        Ok(format!("hashed:{}", password.expose()))
    }
    async fn verify(&self, password: PlainPassword, hash: String) -> Result<bool, AppError> {
        *self.real.lock().unwrap() += 1;
        Ok(hash == format!("hashed:{}", password.expose()))
    }
    async fn verify_dummy(&self, _password: PlainPassword) -> Result<(), AppError> {
        *self.dummy.lock().unwrap() += 1;
        Ok(())
    }
}

#[derive(Default)]
pub struct FakeTokens(Mutex<u64>);

impl TokenService for FakeTokens {
    fn generate(&self) -> NewToken {
        let mut counter = self.0.lock().unwrap();
        *counter += 1;
        let raw = format!("token-{counter}");
        let digest = self.digest(&raw);
        NewToken { raw, digest }
    }

    fn digest(&self, raw: &str) -> TokenDigest {
        let mut bytes = [0u8; 32];
        for (slot, byte) in bytes.iter_mut().zip(raw.bytes()) {
            *slot = byte;
        }
        TokenDigest(bytes)
    }
}

pub struct FakeClock(Mutex<OffsetDateTime>);

impl FakeClock {
    pub fn advance(&self, by: Duration) {
        *self.0.lock().unwrap() += by;
    }
}

impl Clock for FakeClock {
    fn now(&self) -> OffsetDateTime {
        *self.0.lock().unwrap()
    }
}

/// Counts events per key and ignores time; the real windowing logic is
/// tested on the infrastructure implementation.
#[derive(Default)]
pub struct FakeLimiter(Mutex<HashMap<String, u32>>);

#[async_trait]
impl RateLimiter for FakeLimiter {
    async fn check(
        &self,
        key: &str,
        rule: RateLimitRule,
        _now: OffsetDateTime,
    ) -> Result<RateLimitDecision, AppError> {
        let counts = self.0.lock().unwrap();
        Ok(if counts.get(key).copied().unwrap_or(0) >= rule.max {
            RateLimitDecision::Limited {
                retry_after: rule.window,
            }
        } else {
            RateLimitDecision::Allowed
        })
    }

    async fn record(
        &self,
        key: &str,
        _rule: RateLimitRule,
        _now: OffsetDateTime,
    ) -> Result<(), AppError> {
        *self.0.lock().unwrap().entry(key.to_owned()).or_insert(0) += 1;
        Ok(())
    }

    async fn reset(&self, key: &str) -> Result<(), AppError> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
}

/// Limits high enough that tests not about rate limiting never hit them.
pub fn generous_limits() -> RateLimitConfig {
    let rule = RateLimitRule {
        max: 10_000,
        window: Duration::hours(1),
    };
    RateLimitConfig {
        login_per_ip: rule,
        login_failures_per_user: rule,
        register_per_ip: rule,
    }
}

pub struct Harness {
    pub auth: AuthService,
    pub hasher: Arc<FakeHasher>,
    pub clock: Arc<FakeClock>,
    pub ttl: Duration,
}

impl Harness {
    pub fn new(registration: RegistrationPolicy) -> Self {
        Self::with_limits(registration, generous_limits())
    }

    pub fn with_limits(registration: RegistrationPolicy, rate_limits: RateLimitConfig) -> Self {
        let store = FakeStore::default();
        let hasher = Arc::new(FakeHasher::default());
        let clock = Arc::new(FakeClock(Mutex::new(OffsetDateTime::UNIX_EPOCH)));
        let ttl = Duration::days(30);
        let auth = AuthService::new(
            Arc::new(store.clone()),
            Arc::new(store),
            hasher.clone(),
            Arc::new(FakeTokens::default()),
            clock.clone(),
            Arc::new(FakeLimiter::default()),
            AuthConfig {
                registration,
                session_ttl: ttl,
                rate_limits,
            },
        );
        Self {
            auth,
            hasher,
            clock,
            ttl,
        }
    }
}
