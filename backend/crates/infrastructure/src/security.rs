//! Password hashing, session token generation and the system clock.

use std::sync::Arc;

use argon2::{
    Algorithm, Argon2, Params, Version,
    password_hash::{self, PasswordHasher as _, PasswordVerifier as _, phc::PasswordHash},
};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rustchat_application::{
    error::AppError,
    ports::{Clock, NewToken, PasswordHasher, TokenDigest, TokenService},
};
use rustchat_domain::identity::PlainPassword;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

/// Argon2id memory cost in KiB (OWASP minimum recommendation: 19 MiB).
const DEFAULT_M_COST_KIB: u32 = 19 * 1024;
const DEFAULT_T_COST: u32 = 2;
const DEFAULT_P_COST: u32 = 1;

/// Argon2id password hasher. Work runs on the blocking thread pool so it
/// cannot stall the async runtime.
pub struct Argon2Hasher {
    argon2: Arc<Argon2<'static>>,
    dummy_hash: Arc<str>,
}

impl Argon2Hasher {
    pub fn new() -> Result<Self, AppError> {
        Self::with_costs(DEFAULT_M_COST_KIB, DEFAULT_T_COST, DEFAULT_P_COST)
    }

    pub fn with_costs(m_cost_kib: u32, t_cost: u32, p_cost: u32) -> Result<Self, AppError> {
        let params = Params::new(m_cost_kib, t_cost, p_cost, None).map_err(AppError::internal)?;
        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

        // A hash of random data with the same parameters, used to equalize the
        // cost of logins for unknown accounts.
        let mut filler = [0u8; 24];
        getrandom::fill(&mut filler).map_err(AppError::internal)?;
        let dummy_hash = argon2
            .hash_password(URL_SAFE_NO_PAD.encode(filler).as_bytes())
            .map_err(AppError::internal)?
            .to_string();

        Ok(Self {
            argon2: Arc::new(argon2),
            dummy_hash: dummy_hash.into(),
        })
    }
}

fn verify_blocking(argon2: &Argon2<'_>, password: &str, hash: &str) -> Result<bool, AppError> {
    let parsed = PasswordHash::new(hash).map_err(AppError::internal)?;
    match argon2.verify_password(password.as_bytes(), &parsed) {
        Ok(()) => Ok(true),
        Err(password_hash::Error::PasswordInvalid) => Ok(false),
        Err(error) => Err(AppError::internal(error)),
    }
}

#[async_trait]
impl PasswordHasher for Argon2Hasher {
    async fn hash(&self, password: PlainPassword) -> Result<String, AppError> {
        let argon2 = Arc::clone(&self.argon2);
        tokio::task::spawn_blocking(move || {
            argon2
                .hash_password(password.expose().as_bytes())
                .map(|hash| hash.to_string())
                .map_err(AppError::internal)
        })
        .await
        .map_err(AppError::internal)?
    }

    async fn verify(&self, password: PlainPassword, hash: String) -> Result<bool, AppError> {
        let argon2 = Arc::clone(&self.argon2);
        tokio::task::spawn_blocking(move || verify_blocking(&argon2, password.expose(), &hash))
            .await
            .map_err(AppError::internal)?
    }

    async fn verify_dummy(&self, password: PlainPassword) -> Result<(), AppError> {
        let argon2 = Arc::clone(&self.argon2);
        let dummy = Arc::clone(&self.dummy_hash);
        tokio::task::spawn_blocking(move || {
            verify_blocking(&argon2, password.expose(), &dummy).map(|_| ())
        })
        .await
        .map_err(AppError::internal)?
    }
}

/// 256-bit random session tokens from the operating system's CSPRNG.
#[derive(Default)]
pub struct OsTokenService;

impl TokenService for OsTokenService {
    fn generate(&self) -> NewToken {
        let mut bytes = [0u8; 32];
        // Without OS randomness no secure token can be issued; there is no
        // safe fallback, so this is treated as unrecoverable.
        getrandom::fill(&mut bytes).expect("operating system random number generator failed");
        let raw = URL_SAFE_NO_PAD.encode(bytes);
        let digest = self.digest(&raw);
        NewToken { raw, digest }
    }

    fn digest(&self, raw: &str) -> TokenDigest {
        TokenDigest(Sha256::digest(raw.as_bytes()).into())
    }
}

pub struct SystemClock;

impl Clock for SystemClock {
    /// Truncated to microseconds, the precision PostgreSQL stores, so a value
    /// reads back identically to how it was created.
    fn now(&self) -> OffsetDateTime {
        let now = OffsetDateTime::now_utc();
        now.replace_nanosecond(now.nanosecond() / 1000 * 1000)
            .unwrap_or(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Low costs keep tests fast; production uses `Argon2Hasher::new`.
    fn fast_hasher() -> Argon2Hasher {
        Argon2Hasher::with_costs(8, 1, 1).unwrap()
    }

    fn password(raw: &str) -> PlainPassword {
        PlainPassword::parse_new(raw.to_owned()).unwrap()
    }

    #[tokio::test]
    async fn hashes_are_argon2id_and_salted() {
        let hasher = fast_hasher();
        let a = hasher
            .hash(password("correct horse battery"))
            .await
            .unwrap();
        let b = hasher
            .hash(password("correct horse battery"))
            .await
            .unwrap();
        assert!(a.starts_with("$argon2id$"), "unexpected format: {a}");
        assert_ne!(a, b, "salts must differ");
        assert!(!a.contains("correct horse"));
    }

    #[tokio::test]
    async fn verify_accepts_right_and_rejects_wrong_password() {
        let hasher = fast_hasher();
        let hash = hasher
            .hash(password("correct horse battery"))
            .await
            .unwrap();
        assert!(
            hasher
                .verify(password("correct horse battery"), hash.clone())
                .await
                .unwrap()
        );
        assert!(
            !hasher
                .verify(password("incorrect horse battery"), hash)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn malformed_stored_hash_is_an_error_not_a_match() {
        let hasher = fast_hasher();
        let result = hasher
            .verify(password("correct horse battery"), "not-a-hash".into())
            .await;
        assert!(matches!(result, Err(AppError::Internal(_))));
    }

    #[tokio::test]
    async fn dummy_verification_succeeds_without_matching() {
        let hasher = fast_hasher();
        hasher
            .verify_dummy(password("correct horse battery"))
            .await
            .unwrap();
    }

    #[test]
    fn clock_has_microsecond_precision_like_postgres() {
        assert_eq!(SystemClock.now().nanosecond() % 1000, 0);
    }

    #[test]
    fn tokens_are_unique_and_digest_matches() {
        let service = OsTokenService;
        let a = service.generate();
        let b = service.generate();
        assert_ne!(a.raw, b.raw);
        assert_eq!(a.raw.len(), 43, "256 bits as unpadded base64url");
        assert_eq!(a.digest, service.digest(&a.raw));
        assert_ne!(a.digest, b.digest);
    }
}
