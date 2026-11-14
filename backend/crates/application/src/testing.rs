//! In-memory fakes for application unit tests.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rustchat_domain::identity::{PlainPassword, Session, SessionId, User, UserId, Username};
use time::{Duration, OffsetDateTime};

use crate::{
    auth::{AuthConfig, AuthService, RegistrationPolicy},
    error::AppError,
    ports::*,
};

#[derive(Default)]
struct Store {
    users: Vec<(User, String)>,
    sessions: Vec<(Session, TokenDigest, bool)>,
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
            .find(|(u, _)| &u.username == username)
            .map(|(u, hash)| UserCredentials {
                user: u.clone(),
                password_hash: hash.clone(),
            }))
    }
}

#[async_trait]
impl SessionRepository for FakeStore {
    async fn insert(&self, new: NewSession) -> Result<(), AppError> {
        self.0
            .lock()
            .unwrap()
            .sessions
            .push((new.session, new.token_digest, false));
        Ok(())
    }

    async fn find_active(
        &self,
        digest: &TokenDigest,
        now: OffsetDateTime,
    ) -> Result<Option<ActiveSession>, AppError> {
        let store = self.0.lock().unwrap();
        let found = store
            .sessions
            .iter()
            .find(|(s, d, revoked)| d == digest && !revoked && !s.is_expired_at(now));
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
}

#[derive(Default)]
pub struct FakeHasher {
    dummy: Mutex<usize>,
}

impl FakeHasher {
    pub fn dummy_verifications(&self) -> usize {
        *self.dummy.lock().unwrap()
    }
}

#[async_trait]
impl PasswordHasher for FakeHasher {
    async fn hash(&self, password: PlainPassword) -> Result<String, AppError> {
        Ok(format!("hashed:{}", password.expose()))
    }
    async fn verify(&self, password: PlainPassword, hash: String) -> Result<bool, AppError> {
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

pub struct Harness {
    pub auth: AuthService,
    pub hasher: Arc<FakeHasher>,
    pub clock: Arc<FakeClock>,
    pub ttl: Duration,
}

impl Harness {
    pub fn new(registration: RegistrationPolicy) -> Self {
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
            AuthConfig {
                registration,
                session_ttl: ttl,
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
