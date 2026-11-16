//! Time-based one-time-password (TOTP) multi-factor authentication.
//!
//! Enrolment is two-step: the owner re-enters their password to receive a
//! secret, then proves their authenticator works by entering a valid code,
//! which also issues one-time recovery codes. Once confirmed, login needs the
//! password and then a TOTP code (or a recovery code).

use super::*;
use crate::ports::{MfaRepository, TokenDigest, TotpEngine};

/// A password-verified login waits for its second factor this long.
const CHALLENGE_TTL: Duration = Duration::minutes(5);
/// Code guesses allowed on one challenge before it must be restarted.
const CHALLENGE_MAX_ATTEMPTS: i32 = 5;
/// Recovery codes issued at a time.
const RECOVERY_CODE_COUNT: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MfaStatus {
    /// The server has an encryption key configured, so enrolment is possible.
    pub available: bool,
    pub enabled: bool,
    pub recovery_codes_remaining: u32,
}

/// What an authenticator app needs to add the account. Both values are secret.
pub struct TotpEnrollment {
    pub secret_base32: String,
    pub provisioning_uri: String,
}

impl std::fmt::Debug for TotpEnrollment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TotpEnrollment")
            .field("secret_base32", &"<redacted>")
            .field("provisioning_uri", &"<redacted>")
            .finish()
    }
}

/// The result of the password step of a login.
pub enum LoginOutcome {
    Session(LoginResult),
    /// The account has a second factor; finish with [`AuthService::complete_mfa_login`].
    MfaRequired {
        challenge: String,
    },
}

impl std::fmt::Debug for LoginOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Session(result) => f.debug_tuple("Session").field(result).finish(),
            Self::MfaRequired { .. } => f
                .debug_struct("MfaRequired")
                .field("challenge", &"<redacted>")
                .finish(),
        }
    }
}

impl AuthService {
    /// Enable TOTP. Without this, MFA is reported unavailable and login is unchanged.
    pub fn with_mfa(mut self, repo: Arc<dyn MfaRepository>, engine: Arc<dyn TotpEngine>) -> Self {
        self.mfa = Some((repo, engine));
        self
    }

    fn mfa(&self) -> Result<(&dyn MfaRepository, &dyn TotpEngine), AppError> {
        self.mfa
            .as_ref()
            .map(|(repo, engine)| (repo.as_ref(), engine.as_ref()))
            .ok_or(AppError::MfaUnavailable)
    }

    pub async fn mfa_status(&self, user_id: UserId) -> Result<MfaStatus, AppError> {
        let Some((repo, _)) = self.mfa.as_ref() else {
            return Ok(MfaStatus {
                available: false,
                enabled: false,
                recovery_codes_remaining: 0,
            });
        };
        let enabled = repo.totp(user_id).await?.is_some_and(|row| row.confirmed);
        Ok(MfaStatus {
            available: true,
            enabled,
            recovery_codes_remaining: if enabled {
                repo.recovery_codes_remaining(user_id).await?
            } else {
                0
            },
        })
    }

    /// Whether the password step must be followed by a second factor.
    pub(super) async fn requires_second_factor(&self, user_id: UserId) -> Result<bool, AppError> {
        match &self.mfa {
            Some((repo, _)) => Ok(repo.totp(user_id).await?.is_some_and(|row| row.confirmed)),
            None => Ok(false),
        }
    }

    /// Park a password-verified login until the second factor arrives.
    pub(super) async fn start_mfa_challenge(&self, user_id: UserId) -> Result<String, AppError> {
        let (repo, _) = self.mfa()?;
        let now = self.clock.now();
        let token = self.tokens.generate();
        repo.create_challenge(user_id, token.digest, now, now + CHALLENGE_TTL)
            .await?;
        Ok(token.raw)
    }

    /// Finish a login that needed a second factor. Every failure looks the same
    /// (`InvalidMfaCode`): wrong code, expired or exhausted challenge, replayed code.
    pub async fn complete_mfa_login(
        &self,
        client_ip: IpAddr,
        challenge: &str,
        code: &str,
    ) -> Result<LoginResult, AppError> {
        let (repo, _) = self.mfa()?;
        if challenge.is_empty() || challenge.len() > 128 {
            return Err(AppError::InvalidMfaCode);
        }
        let now = self.clock.now();
        let ip_rule = self.config.rate_limits.login_per_ip;
        let ip_key = login_ip_key(client_ip).replace("login:", "mfa:");
        self.enforce(&ip_key, ip_rule, now).await?;
        self.limiter.record(&ip_key, ip_rule, now).await?;

        let digest = self.tokens.digest(challenge);
        let user = repo
            .attempt_challenge(&digest, now, CHALLENGE_MAX_ATTEMPTS)
            .await?
            .ok_or(AppError::InvalidMfaCode)?;
        self.verify_second_factor_limited(user.id, code, now)
            .await?;
        repo.delete_challenge(&digest).await?;
        self.start_session(user).await
    }

    /// Start enrolment: returns the secret to add to an authenticator app. The
    /// secret only takes effect once a code from it is confirmed.
    pub async fn begin_totp_enrollment(
        &self,
        auth: &AuthContext,
        client_ip: IpAddr,
        current_password: String,
    ) -> Result<TotpEnrollment, AppError> {
        let (repo, engine) = self.mfa()?;
        self.verify_current_password(auth, client_ip, current_password)
            .await?;
        if repo
            .totp(auth.user.id)
            .await?
            .is_some_and(|row| row.confirmed)
        {
            return Err(AppError::MfaAlreadyEnabled);
        }
        let secret = engine.generate_secret()?;
        let sealed = engine.seal(auth.user.id, &secret)?;
        if !repo
            .begin_totp(auth.user.id, &sealed, self.clock.now())
            .await?
        {
            return Err(AppError::MfaAlreadyEnabled);
        }
        Ok(TotpEnrollment {
            secret_base32: engine.base32(&secret),
            provisioning_uri: engine.provisioning_uri(&secret, auth.user.username.as_str())?,
        })
    }

    /// Finish enrolment with a code from the authenticator. Returns the one-time
    /// recovery codes, which are shown only now.
    pub async fn confirm_totp_enrollment(
        &self,
        auth: &AuthContext,
        code: &str,
    ) -> Result<Vec<String>, AppError> {
        let (repo, engine) = self.mfa()?;
        let now = self.clock.now();
        let user_key = mfa_user_key(auth.user.id);
        let rule = self.config.rate_limits.login_failures_per_user;
        self.enforce(&user_key, rule, now).await?;

        let record = repo
            .totp(auth.user.id)
            .await?
            .filter(|row| !row.confirmed)
            .ok_or(AppError::InvalidMfaCode)?;
        let secret = engine.open(auth.user.id, &record.secret)?;
        let Some(step) = engine.matching_step(&secret, code.trim(), now) else {
            self.limiter.record(&user_key, rule, now).await?;
            return Err(AppError::InvalidMfaCode);
        };
        let (codes, digests) = self.new_recovery_codes();
        if !repo.confirm_totp(auth.user.id, step, &digests, now).await? {
            return Err(AppError::InvalidMfaCode);
        }
        self.limiter.reset(&user_key).await?;
        Ok(codes)
    }

    /// Turn the second factor off. Needs the password and a current code, so a
    /// stolen session alone cannot remove it.
    pub async fn disable_mfa(
        &self,
        auth: &AuthContext,
        client_ip: IpAddr,
        current_password: String,
        code: &str,
    ) -> Result<(), AppError> {
        let (repo, _) = self.mfa()?;
        self.verify_current_password(auth, client_ip, current_password)
            .await?;
        self.require_confirmed_second_factor(auth.user.id, code)
            .await?;
        repo.disable_totp(auth.user.id).await
    }

    /// Replace the recovery codes; the old ones stop working.
    pub async fn regenerate_recovery_codes(
        &self,
        auth: &AuthContext,
        client_ip: IpAddr,
        current_password: String,
        code: &str,
    ) -> Result<Vec<String>, AppError> {
        let (repo, _) = self.mfa()?;
        self.verify_current_password(auth, client_ip, current_password)
            .await?;
        self.require_confirmed_second_factor(auth.user.id, code)
            .await?;
        let (codes, digests) = self.new_recovery_codes();
        repo.replace_recovery_codes(auth.user.id, &digests, self.clock.now())
            .await?;
        Ok(codes)
    }

    async fn require_confirmed_second_factor(
        &self,
        user_id: UserId,
        code: &str,
    ) -> Result<(), AppError> {
        let (repo, _) = self.mfa()?;
        if !repo.totp(user_id).await?.is_some_and(|row| row.confirmed) {
            return Err(AppError::InvalidMfaCode);
        }
        self.verify_second_factor_limited(user_id, code, self.clock.now())
            .await
    }

    fn new_recovery_codes(&self) -> (Vec<String>, Vec<TokenDigest>) {
        let codes: Vec<String> = (0..RECOVERY_CODE_COUNT)
            .map(|_| self.tokens.recovery_code())
            .collect();
        let digests = codes
            .iter()
            .map(|code| self.tokens.digest(&normalize_recovery_code(code)))
            .collect();
        (codes, digests)
    }

    /// Check a code under the per-account failure budget; failures spend it.
    async fn verify_second_factor_limited(
        &self,
        user_id: UserId,
        code: &str,
        now: OffsetDateTime,
    ) -> Result<(), AppError> {
        let user_key = mfa_user_key(user_id);
        let rule = self.config.rate_limits.login_failures_per_user;
        self.enforce(&user_key, rule, now).await?;
        if self.verify_second_factor(user_id, code, now).await? {
            self.limiter.reset(&user_key).await?;
            Ok(())
        } else {
            self.limiter.record(&user_key, rule, now).await?;
            Err(AppError::InvalidMfaCode)
        }
    }

    /// A six-digit input is a TOTP code, anything else a recovery code. Each
    /// time step and each recovery code is accepted once.
    async fn verify_second_factor(
        &self,
        user_id: UserId,
        code: &str,
        now: OffsetDateTime,
    ) -> Result<bool, AppError> {
        let (repo, engine) = self.mfa()?;
        let code = code.trim();
        if code.len() == 6 && code.bytes().all(|byte| byte.is_ascii_digit()) {
            let Some(record) = repo.totp(user_id).await?.filter(|row| row.confirmed) else {
                return Ok(false);
            };
            let secret = engine.open(user_id, &record.secret)?;
            let Some(step) = engine.matching_step(&secret, code, now) else {
                return Ok(false);
            };
            return repo.use_totp_step(user_id, step).await;
        }
        let normalized = normalize_recovery_code(code);
        if normalized.is_empty() || normalized.len() > 64 {
            return Ok(false);
        }
        repo.use_recovery_code(user_id, &self.tokens.digest(&normalized), now)
            .await
    }
}

fn mfa_user_key(user_id: UserId) -> String {
    format!("mfa:user:{}", user_id.as_uuid())
}

/// Recovery codes are shown in groups; accept them with or without separators and in any case.
fn normalize_recovery_code(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}
