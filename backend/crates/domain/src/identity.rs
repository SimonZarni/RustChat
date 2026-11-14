//! Identity concepts: users, sessions and the rules for their inputs.

use std::fmt;
use time::OffsetDateTime;
use uuid::Uuid;

macro_rules! id_type {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name(Uuid);

        impl $name {
            /// Generate a new time-ordered (UUIDv7) identifier.
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            pub fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }

            pub fn as_uuid(&self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

id_type!(UserId);
id_type!(SessionId);

pub const USERNAME_MIN_LEN: usize = 3;
pub const USERNAME_MAX_LEN: usize = 32;
pub const PASSWORD_MIN_LEN: usize = 12;
pub const PASSWORD_MAX_LEN: usize = 128;

/// A normalized (lower-case ASCII) username: `[a-z0-9_]`, 3 to 32 characters.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Username(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UsernameError {
    #[error("username must be at least {USERNAME_MIN_LEN} characters")]
    TooShort,
    #[error("username must be at most {USERNAME_MAX_LEN} characters")]
    TooLong,
    #[error("username may only contain letters, digits and underscores")]
    InvalidCharacter,
}

impl Username {
    /// Validate and normalize a username. Matching is case-insensitive.
    pub fn parse(raw: &str) -> Result<Self, UsernameError> {
        let len = raw.chars().count();
        if len < USERNAME_MIN_LEN {
            return Err(UsernameError::TooShort);
        }
        if len > USERNAME_MAX_LEN {
            return Err(UsernameError::TooLong);
        }
        let normalized = raw.to_ascii_lowercase();
        if !normalized
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            return Err(UsernameError::InvalidCharacter);
        }
        Ok(Self(normalized))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Username {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A plaintext password. Never logged: `Debug` is redacted.
#[derive(Clone, PartialEq, Eq)]
pub struct PlainPassword(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PasswordError {
    #[error("password must be at least {PASSWORD_MIN_LEN} characters")]
    TooShort,
    #[error("password must be at most {PASSWORD_MAX_LEN} characters")]
    TooLong,
}

impl PlainPassword {
    /// Apply the password policy for a new password.
    pub fn parse_new(raw: String) -> Result<Self, PasswordError> {
        let len = raw.chars().count();
        if len < PASSWORD_MIN_LEN {
            return Err(PasswordError::TooShort);
        }
        if len > PASSWORD_MAX_LEN {
            return Err(PasswordError::TooLong);
        }
        Ok(Self(raw))
    }

    /// Accept a password supplied at login. Only bounds the length so that
    /// verification work stays bounded; the policy is not re-applied, so
    /// the result does not reveal whether an account exists.
    pub fn from_login_input(raw: String) -> Option<Self> {
        if raw.is_empty() || raw.chars().count() > PASSWORD_MAX_LEN {
            None
        } else {
            Some(Self(raw))
        }
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for PlainPassword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PlainPassword(<redacted>)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub id: UserId,
    pub username: Username,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: SessionId,
    pub user_id: UserId,
    pub created_at: OffsetDateTime,
    pub expires_at: OffsetDateTime,
}

impl Session {
    pub fn is_expired_at(&self, now: OffsetDateTime) -> bool {
        now >= self.expires_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn username_is_normalized_to_lowercase() {
        assert_eq!(Username::parse("Alice_01").unwrap().as_str(), "alice_01");
    }

    #[test]
    fn username_rejects_bad_input() {
        assert_eq!(Username::parse("ab"), Err(UsernameError::TooShort));
        assert_eq!(
            Username::parse(&"a".repeat(33)),
            Err(UsernameError::TooLong)
        );
        assert_eq!(
            Username::parse("bad name"),
            Err(UsernameError::InvalidCharacter)
        );
        assert_eq!(
            Username::parse("émile"),
            Err(UsernameError::InvalidCharacter)
        );
        assert_eq!(
            Username::parse("a-b-c"),
            Err(UsernameError::InvalidCharacter)
        );
    }

    #[test]
    fn username_accepts_boundary_lengths() {
        assert!(Username::parse(&"a".repeat(3)).is_ok());
        assert!(Username::parse(&"a".repeat(32)).is_ok());
    }

    #[test]
    fn new_password_policy_is_enforced() {
        assert_eq!(
            PlainPassword::parse_new("short".into()),
            Err(PasswordError::TooShort)
        );
        assert_eq!(
            PlainPassword::parse_new("x".repeat(129)),
            Err(PasswordError::TooLong)
        );
        assert!(PlainPassword::parse_new("x".repeat(12)).is_ok());
        assert!(PlainPassword::parse_new("x".repeat(128)).is_ok());
    }

    #[test]
    fn login_input_only_bounds_length() {
        assert!(PlainPassword::from_login_input("short".into()).is_some());
        assert!(PlainPassword::from_login_input(String::new()).is_none());
        assert!(PlainPassword::from_login_input("x".repeat(129)).is_none());
    }

    #[test]
    fn password_debug_is_redacted() {
        let password = PlainPassword::parse_new("correct horse battery".into()).unwrap();
        assert!(!format!("{password:?}").contains("correct"));
    }

    #[test]
    fn session_expiry_is_inclusive_of_the_expiry_instant() {
        let now = OffsetDateTime::UNIX_EPOCH;
        let session = Session {
            id: SessionId::new(),
            user_id: UserId::new(),
            created_at: now,
            expires_at: now + time::Duration::hours(1),
        };
        assert!(!session.is_expired_at(now));
        assert!(session.is_expired_at(session.expires_at));
    }

    #[test]
    fn ids_are_unique() {
        assert_ne!(UserId::new(), UserId::new());
    }
}
