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

/// Activity and token-rotation bookkeeping for a fake session.
struct SessionMeta {
    last_seen_at: OffsetDateTime,
    token_rotated_at: OffsetDateTime,
    previous: Option<(TokenDigest, OffsetDateTime)>,
}

struct VerificationToken {
    digest: TokenDigest,
    user_id: UserId,
    email: rustchat_domain::identity::EmailAddress,
    expires_at: OffsetDateTime,
    used: bool,
}

struct Challenge {
    digest: TokenDigest,
    user_id: UserId,
    expires_at: OffsetDateTime,
    attempts: i32,
}

#[derive(Default)]
struct Store {
    totp: HashMap<UserId, TotpRecord>,
    recovery_codes: Vec<(UserId, TokenDigest, bool)>,
    challenges: Vec<Challenge>,
    emails: HashMap<
        UserId,
        (
            rustchat_domain::identity::EmailAddress,
            Option<OffsetDateTime>,
        ),
    >,
    verification_tokens: Vec<VerificationToken>,
    users: Vec<(User, String)>,
    sessions: Vec<(Session, TokenDigest, bool)>,
    session_meta: HashMap<SessionId, SessionMeta>,
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
        if let Some(email) = user.email {
            store.emails.insert(record.id, (email, None));
        }
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

    async fn email_status(&self, user_id: UserId) -> Result<EmailStatus, AppError> {
        let store = self.0.lock().unwrap();
        Ok(match store.emails.get(&user_id) {
            Some((address, verified_at)) => EmailStatus {
                address: Some(address.clone()),
                verified: verified_at.is_some(),
            },
            None => EmailStatus {
                address: None,
                verified: false,
            },
        })
    }

    async fn set_email(
        &self,
        user_id: UserId,
        email: Option<&rustchat_domain::identity::EmailAddress>,
    ) -> Result<(), AppError> {
        let mut store = self.0.lock().unwrap();
        match email {
            Some(email) => {
                store.emails.insert(user_id, (email.clone(), None));
            }
            None => {
                store.emails.remove(&user_id);
            }
        }
        store.verification_tokens.retain(|t| t.user_id != user_id);
        Ok(())
    }

    async fn issue_email_verification_token(
        &self,
        user_id: UserId,
        email: &rustchat_domain::identity::EmailAddress,
        digest: TokenDigest,
        _now: OffsetDateTime,
        expires_at: OffsetDateTime,
    ) -> Result<bool, AppError> {
        let mut store = self.0.lock().unwrap();
        match store.emails.get(&user_id) {
            Some((current, None)) if current == email => {}
            _ => return Ok(false),
        }
        store
            .verification_tokens
            .retain(|t| t.user_id != user_id || t.used);
        store.verification_tokens.push(VerificationToken {
            digest,
            user_id,
            email: email.clone(),
            expires_at,
            used: false,
        });
        Ok(true)
    }

    async fn verify_email_with_token(
        &self,
        digest: TokenDigest,
        now: OffsetDateTime,
    ) -> Result<VerifyEmailOutcome, AppError> {
        let mut store = self.0.lock().unwrap();
        let Some(index) = store
            .verification_tokens
            .iter()
            .position(|t| t.digest == digest && !t.used && t.expires_at > now)
        else {
            return Ok(VerifyEmailOutcome::Invalid);
        };
        let (user_id, email) = {
            let token = &store.verification_tokens[index];
            (token.user_id, token.email.clone())
        };
        let taken = store.emails.iter().any(|(id, (address, verified))| {
            *id != user_id && *address == email && verified.is_some()
        });
        if taken {
            return Ok(VerifyEmailOutcome::AddressInUse);
        }
        store.verification_tokens[index].used = true;
        match store.emails.get_mut(&user_id) {
            Some((current, verified @ None)) if *current == email => {
                *verified = Some(now);
                Ok(VerifyEmailOutcome::Verified)
            }
            _ => Ok(VerifyEmailOutcome::Invalid),
        }
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

    async fn list_admin_audit(
        &self,
        _before: Option<rustchat_domain::identity::AuditEventId>,
        _limit: usize,
    ) -> Result<AuditPage, AppError> {
        Ok(AuditPage {
            events: Vec::new(),
            next_cursor: None,
        })
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
impl MfaRepository for FakeStore {
    async fn totp(&self, user_id: UserId) -> Result<Option<TotpRecord>, AppError> {
        Ok(self.0.lock().unwrap().totp.get(&user_id).cloned())
    }

    async fn begin_totp(
        &self,
        user_id: UserId,
        secret: &EncryptedSecret,
        _now: OffsetDateTime,
    ) -> Result<bool, AppError> {
        let mut store = self.0.lock().unwrap();
        if store.totp.get(&user_id).is_some_and(|row| row.confirmed) {
            return Ok(false);
        }
        store.totp.insert(
            user_id,
            TotpRecord {
                secret: secret.clone(),
                confirmed: false,
                last_used_step: None,
            },
        );
        Ok(true)
    }

    async fn confirm_totp(
        &self,
        user_id: UserId,
        step: i64,
        recovery_digests: &[TokenDigest],
        _now: OffsetDateTime,
    ) -> Result<bool, AppError> {
        let mut store = self.0.lock().unwrap();
        match store.totp.get_mut(&user_id) {
            Some(row) if !row.confirmed => {
                row.confirmed = true;
                row.last_used_step = Some(step);
            }
            _ => return Ok(false),
        }
        store.recovery_codes.retain(|(id, _, _)| *id != user_id);
        store.recovery_codes.extend(
            recovery_digests
                .iter()
                .map(|digest| (user_id, *digest, false)),
        );
        Ok(true)
    }

    async fn use_totp_step(&self, user_id: UserId, step: i64) -> Result<bool, AppError> {
        let mut store = self.0.lock().unwrap();
        match store.totp.get_mut(&user_id) {
            Some(row) if row.confirmed && row.last_used_step.is_none_or(|last| last < step) => {
                row.last_used_step = Some(step);
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    async fn use_recovery_code(
        &self,
        user_id: UserId,
        digest: &TokenDigest,
        _now: OffsetDateTime,
    ) -> Result<bool, AppError> {
        let mut store = self.0.lock().unwrap();
        match store
            .recovery_codes
            .iter_mut()
            .find(|(id, d, used)| *id == user_id && d == digest && !*used)
        {
            Some(entry) => {
                entry.2 = true;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    async fn recovery_codes_remaining(&self, user_id: UserId) -> Result<u32, AppError> {
        let store = self.0.lock().unwrap();
        Ok(store
            .recovery_codes
            .iter()
            .filter(|(id, _, used)| *id == user_id && !*used)
            .count() as u32)
    }

    async fn replace_recovery_codes(
        &self,
        user_id: UserId,
        digests: &[TokenDigest],
        _now: OffsetDateTime,
    ) -> Result<(), AppError> {
        let mut store = self.0.lock().unwrap();
        store.recovery_codes.retain(|(id, _, _)| *id != user_id);
        store
            .recovery_codes
            .extend(digests.iter().map(|digest| (user_id, *digest, false)));
        Ok(())
    }

    async fn disable_totp(&self, user_id: UserId) -> Result<(), AppError> {
        let mut store = self.0.lock().unwrap();
        store.totp.remove(&user_id);
        store.recovery_codes.retain(|(id, _, _)| *id != user_id);
        store.challenges.retain(|c| c.user_id != user_id);
        Ok(())
    }

    async fn create_challenge(
        &self,
        user_id: UserId,
        digest: TokenDigest,
        _now: OffsetDateTime,
        expires_at: OffsetDateTime,
    ) -> Result<(), AppError> {
        self.0.lock().unwrap().challenges.push(Challenge {
            digest,
            user_id,
            expires_at,
            attempts: 0,
        });
        Ok(())
    }

    async fn attempt_challenge(
        &self,
        digest: &TokenDigest,
        now: OffsetDateTime,
        max_attempts: i32,
    ) -> Result<Option<User>, AppError> {
        let mut store = self.0.lock().unwrap();
        let Some(challenge) = store
            .challenges
            .iter_mut()
            .find(|c| &c.digest == digest && c.expires_at > now && c.attempts < max_attempts)
        else {
            return Ok(None);
        };
        challenge.attempts += 1;
        let user_id = challenge.user_id;
        Ok(store
            .users
            .iter()
            .find(|(user, _)| user.id == user_id)
            .map(|(user, _)| user.clone()))
    }

    async fn delete_challenge(&self, digest: &TokenDigest) -> Result<(), AppError> {
        self.0
            .lock()
            .unwrap()
            .challenges
            .retain(|c| &c.digest != digest);
        Ok(())
    }
}

/// A TOTP engine with predictable codes: the code for a 30-second step is
/// `FakeTotp::code_at(time)`. It does no cryptography.
pub struct FakeTotp;

impl FakeTotp {
    fn code_for_step(step: i64) -> String {
        format!("{:06}", (step * 7919 + 12_345).rem_euclid(1_000_000))
    }

    pub fn code_at(time: OffsetDateTime) -> String {
        Self::code_for_step(time.unix_timestamp() / 30)
    }
}

impl TotpEngine for FakeTotp {
    fn generate_secret(&self) -> Result<Vec<u8>, AppError> {
        Ok(vec![7; 20])
    }

    fn seal(&self, _user_id: UserId, secret: &[u8]) -> Result<EncryptedSecret, AppError> {
        Ok(EncryptedSecret {
            ciphertext: secret.to_vec(),
            nonce: [0; 12],
        })
    }

    fn open(&self, _user_id: UserId, sealed: &EncryptedSecret) -> Result<Vec<u8>, AppError> {
        Ok(sealed.ciphertext.clone())
    }

    fn base32(&self, _secret: &[u8]) -> String {
        "FAKESECRET".to_owned()
    }

    fn provisioning_uri(&self, _secret: &[u8], account: &str) -> Result<String, AppError> {
        Ok(format!(
            "otpauth://totp/RustChat:{account}?secret=FAKESECRET"
        ))
    }

    fn matching_step(&self, _secret: &[u8], code: &str, now: OffsetDateTime) -> Option<i64> {
        let current = now.unix_timestamp() / 30;
        (current - 1..=current + 1).find(|step| Self::code_for_step(*step) == code)
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
        store.session_meta.insert(
            new.session.id,
            SessionMeta {
                last_seen_at: new.session.created_at,
                token_rotated_at: new.session.created_at,
                previous: None,
            },
        );
        store.sessions.push((new.session, new.token_digest, false));
        Ok(())
    }

    async fn touch(&self, session_id: SessionId, now: OffsetDateTime) -> Result<(), AppError> {
        if let Some(meta) = self.0.lock().unwrap().session_meta.get_mut(&session_id) {
            meta.last_seen_at = now;
        }
        Ok(())
    }

    async fn rotate_token(
        &self,
        session_id: SessionId,
        old_digest: &TokenDigest,
        new_digest: &TokenDigest,
        now: OffsetDateTime,
        grace_until: OffsetDateTime,
    ) -> Result<bool, AppError> {
        let mut store = self.0.lock().unwrap();
        let Some(entry) = store
            .sessions
            .iter_mut()
            .find(|(s, d, revoked)| s.id == session_id && d == old_digest && !revoked)
        else {
            return Ok(false);
        };
        entry.1 = *new_digest;
        let meta = store.session_meta.get_mut(&session_id).unwrap();
        meta.last_seen_at = now;
        meta.token_rotated_at = now;
        meta.previous = Some((*old_digest, grace_until));
        Ok(true)
    }

    async fn find_active(
        &self,
        digest: &TokenDigest,
        now: OffsetDateTime,
    ) -> Result<Option<ActiveSession>, AppError> {
        let store = self.0.lock().unwrap();
        let found = store.sessions.iter().find_map(|(s, d, revoked)| {
            let meta = store.session_meta.get(&s.id)?;
            let via_previous = meta
                .previous
                .is_some_and(|(previous, until)| &previous == digest && now < until);
            ((d == digest || via_previous)
                && !revoked
                && !s.is_expired_at(now)
                && !store.deactivated.contains(&s.user_id)
                && !store.suspended.contains_key(&s.user_id)
                && !store.deleted.contains(&s.user_id))
            .then_some((s, meta, via_previous))
        });
        Ok(found.and_then(|(s, meta, via_previous_token)| {
            store
                .users
                .iter()
                .find(|(u, _)| u.id == s.user_id)
                .map(|(u, _)| ActiveSession {
                    session: s.clone(),
                    user: u.clone(),
                    last_seen_at: meta.last_seen_at,
                    token_rotated_at: meta.token_rotated_at,
                    via_previous_token,
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

    fn recovery_code(&self) -> String {
        let mut counter = self.0.lock().unwrap();
        *counter += 1;
        format!("rc{:02}-aaaa-bbbb-cccc", *counter)
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
    pub fn new(start: OffsetDateTime) -> Self {
        Self(Mutex::new(start))
    }

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

/// Records every email the service sends.
#[derive(Default)]
pub struct FakeEmail(Mutex<Vec<(String, String, String)>>);

impl FakeEmail {
    /// `(recipient, subject, body)` of every message sent so far.
    pub fn sent(&self) -> Vec<(String, String, String)> {
        self.0.lock().unwrap().clone()
    }

    /// The token in the most recent link sent to `recipient`.
    pub fn last_token_for(&self, recipient: &str) -> Option<String> {
        let sent = self.0.lock().unwrap();
        let (_, _, body) = sent.iter().rev().find(|(to, _, _)| to == recipient)?;
        Some(
            body.split("token=")
                .nth(1)?
                .split_whitespace()
                .next()?
                .to_owned(),
        )
    }
}

#[async_trait]
impl EmailDelivery for FakeEmail {
    async fn send(
        &self,
        recipient: &rustchat_domain::identity::EmailAddress,
        subject: &str,
        text: &str,
    ) -> Result<(), AppError> {
        self.0.lock().unwrap().push((
            recipient.as_str().to_owned(),
            subject.to_owned(),
            text.to_owned(),
        ));
        Ok(())
    }
}

pub struct Harness {
    pub emails: Arc<FakeEmail>,
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
        let emails = Arc::new(FakeEmail::default());
        let auth = AuthService::new(
            Arc::new(store.clone()),
            Arc::new(store.clone()),
            hasher.clone(),
            Arc::new(FakeTokens::default()),
            clock.clone(),
            Arc::new(FakeLimiter::default()),
            AuthConfig {
                registration,
                session_ttl: ttl,
                rate_limits,
            },
        )
        .with_email_delivery(emails.clone(), "https://chat.example.test".to_owned())
        .with_mfa(Arc::new(store), Arc::new(FakeTotp));
        Self {
            emails,
            auth,
            hasher,
            clock,
            ttl,
        }
    }

    pub fn with_lifecycle(mut self, lifecycle: crate::auth::SessionLifecycle) -> Self {
        self.auth = self.auth.with_session_lifecycle(lifecycle);
        self
    }
}
