//! TOTP and secret sealing for multi-factor authentication.
//!
//! The one-time-password algorithm (RFC 6238) comes from `totp-rs`, and
//! secrets are sealed with AES-256-GCM from RustCrypto. Nothing here
//! implements cryptography itself.

use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use rustchat_application::{
    error::AppError,
    ports::{EncryptedSecret, TotpEngine},
};
use rustchat_domain::identity::UserId;
use time::OffsetDateTime;
use totp_rs::Totp;

/// Length in bytes of the generated TOTP secret (160 bits, as recommended by RFC 4226).
const SECRET_BYTES: usize = 20;

pub struct AesGcmTotpEngine {
    cipher: Aes256Gcm,
    issuer: String,
}

impl AesGcmTotpEngine {
    /// `key` is the 256-bit server key that protects stored secrets; it must come from
    /// configuration and never from the database. `issuer` names the service in
    /// authenticator apps.
    pub fn new(key: [u8; 32], issuer: impl Into<String>) -> Self {
        Self {
            cipher: Aes256Gcm::new(&Key::<Aes256Gcm>::from(key)),
            issuer: issuer.into(),
        }
    }

    fn totp(&self, secret: &[u8], account: &str) -> Result<Totp, AppError> {
        totp_rs::Builder::new()
            .with_secret(secret.to_vec())
            .with_skew(1)
            .with_account_name(account)
            .with_issuer(Some(self.issuer.as_str()))
            .build()
            .map_err(|_| AppError::internal("invalid TOTP parameters"))
    }
}

impl TotpEngine for AesGcmTotpEngine {
    fn generate_secret(&self) -> Result<Vec<u8>, AppError> {
        let mut secret = vec![0u8; SECRET_BYTES];
        getrandom::fill(&mut secret)
            .map_err(|_| AppError::internal("operating system random number generator failed"))?;
        Ok(secret)
    }

    fn seal(&self, user_id: UserId, secret: &[u8]) -> Result<EncryptedSecret, AppError> {
        // A fresh random 96-bit nonce per secret; with a handful of secrets per key the
        // chance of a repeat is negligible.
        let mut nonce = [0u8; 12];
        getrandom::fill(&mut nonce)
            .map_err(|_| AppError::internal("operating system random number generator failed"))?;
        let ciphertext = self
            .cipher
            .encrypt(
                &Nonce::from(nonce),
                Payload {
                    msg: secret,
                    aad: user_id.as_uuid().as_bytes(),
                },
            )
            .map_err(|_| AppError::internal("could not seal the TOTP secret"))?;
        Ok(EncryptedSecret { ciphertext, nonce })
    }

    fn open(&self, user_id: UserId, sealed: &EncryptedSecret) -> Result<Vec<u8>, AppError> {
        // Binding the user id as associated data means a ciphertext copied to another
        // account does not open.
        self.cipher
            .decrypt(
                &Nonce::from(sealed.nonce),
                Payload {
                    msg: &sealed.ciphertext,
                    aad: user_id.as_uuid().as_bytes(),
                },
            )
            .map_err(|_| AppError::internal("could not open the TOTP secret (wrong key?)"))
    }

    fn base32(&self, secret: &[u8]) -> String {
        totp_rs::Secret::from(secret.to_vec()).to_base32()
    }

    fn provisioning_uri(&self, secret: &[u8], account: &str) -> Result<String, AppError> {
        self.totp(secret, account)?
            .to_url()
            .map_err(|_| AppError::internal("could not build the provisioning URI"))
    }

    fn matching_step(&self, secret: &[u8], code: &str, now: OffsetDateTime) -> Option<i64> {
        let time = u64::try_from(now.unix_timestamp()).ok()?;
        let totp = self.totp(secret, "account").ok()?;
        totp.check(code, time)
            .and_then(|step| i64::try_from(step).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> AesGcmTotpEngine {
        AesGcmTotpEngine::new([7u8; 32], "RustChat")
    }

    const RFC_SECRET: &[u8] = b"12345678901234567890";

    fn at(seconds: i64) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(seconds).unwrap()
    }

    #[test]
    fn matches_the_rfc_6238_sha1_test_vectors() {
        // RFC 6238 appendix B lists 8-digit codes; the six-digit code is their last six digits.
        let engine = engine();
        for (time, code) in [
            (59, "287082"),
            (1_111_111_109, "081804"),
            (20_000_000_000, "353130"),
        ] {
            assert_eq!(
                engine.matching_step(RFC_SECRET, code, at(time)),
                Some(time / 30),
                "time {time}"
            );
        }
    }

    #[test]
    fn accepts_one_step_of_clock_skew_but_no_more() {
        let engine = engine();
        // 287082 is the code for the step containing t = 59 (step 1).
        assert_eq!(
            engine.matching_step(RFC_SECRET, "287082", at(59 + 30)),
            Some(1)
        );
        assert_eq!(engine.matching_step(RFC_SECRET, "287082", at(29)), Some(1));
        assert_eq!(
            engine.matching_step(RFC_SECRET, "287082", at(59 + 90)),
            None
        );
    }

    #[test]
    fn rejects_malformed_and_wrong_codes() {
        let engine = engine();
        for code in ["", "12345", "1234567", "abcdef", "000000", "287 082"] {
            assert_eq!(
                engine.matching_step(RFC_SECRET, code, at(59)),
                None,
                "{code:?}"
            );
        }
    }

    #[test]
    fn secrets_are_random_and_long_enough() {
        let engine = engine();
        let (a, b) = (
            engine.generate_secret().unwrap(),
            engine.generate_secret().unwrap(),
        );
        assert_eq!(a.len(), 20);
        assert_ne!(a, b);
    }

    #[test]
    fn sealed_secrets_round_trip_only_for_their_owner_and_key() {
        let engine = engine();
        let (alice, bobby) = (UserId::new(), UserId::new());
        let secret = engine.generate_secret().unwrap();
        let sealed = engine.seal(alice, &secret).unwrap();

        assert_eq!(engine.open(alice, &sealed).unwrap(), secret);
        assert!(
            !sealed
                .ciphertext
                .windows(secret.len())
                .any(|w| w == secret.as_slice())
        );
        assert!(
            engine.open(bobby, &sealed).is_err(),
            "bound to the owning user"
        );
        let other_key = AesGcmTotpEngine::new([8u8; 32], "RustChat");
        assert!(
            other_key.open(alice, &sealed).is_err(),
            "bound to the server key"
        );

        let mut tampered = sealed.clone();
        tampered.ciphertext[0] ^= 1;
        assert!(engine.open(alice, &tampered).is_err());
        // Two seals of the same secret differ.
        assert_ne!(engine.seal(alice, &secret).unwrap(), sealed);
    }

    #[test]
    fn the_provisioning_uri_names_the_issuer_and_account() {
        let uri = engine().provisioning_uri(RFC_SECRET, "alice").unwrap();
        assert!(uri.starts_with("otpauth://totp/"), "{uri}");
        assert!(uri.contains("RustChat"), "{uri}");
        assert!(uri.contains("alice"), "{uri}");
        assert!(
            uri.contains(&format!("secret={}", engine().base32(RFC_SECRET))),
            "{uri}"
        );
    }
}
