//! Runtime configuration, read from environment variables.

use std::{collections::HashSet, fmt, net::SocketAddr, num::NonZeroUsize};

use rustchat_application::auth::RegistrationPolicy;
use rustchat_application::{
    ports::RateLimitRule,
    ratelimit::{ApiRateLimitConfig, RateLimitConfig},
};
use rustchat_domain::identity::UserId;
use rustchat_http::ClientIpSource;
use uuid::Uuid;

const DEFAULT_BIND: &str = "127.0.0.1:8080";
const DEFAULT_DB_MAX_CONNECTIONS: u32 = 10;
const DEFAULT_GROUP_MEMBER_LIMIT: usize = 100;
const MAX_GROUP_MEMBER_LIMIT: usize = 500;
const DEFAULT_SESSION_IDLE_DAYS: i64 = 7;
const MAX_SESSION_IDLE_DAYS: i64 = 30;
const MAX_RATE_LIMIT: u32 = 100_000;
const DEFAULT_ATTACHMENT_MAX_MIB: u64 = 25;
const MAX_ATTACHMENT_MAX_MIB: u64 = 100;
const DEFAULT_ATTACHMENT_QUOTA_GIB: u64 = 2;

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("RUSTCHAT_BIND is not a valid socket address")]
    InvalidBindAddr,
    #[error("DATABASE_URL is required")]
    MissingDatabaseUrl,
    #[error("RUSTCHAT_REGISTRATION must be `open` or `closed`")]
    InvalidRegistration,
    #[error("RUSTCHAT_ADMIN_USER_IDS must be a comma-separated list of UUIDs")]
    InvalidAdminUserIds,
    #[error(
        "RUSTCHAT_ALLOWED_ORIGINS is required (comma-separated, e.g. https://chat.example.com)"
    )]
    MissingAllowedOrigins,
    #[error("RUSTCHAT_ALLOWED_ORIGINS entries must look like https://host[:port] with no path")]
    InvalidAllowedOrigin,
    #[error("RUSTCHAT_COOKIE_SECURE must be `true` or `false`")]
    InvalidCookieSecure,
    #[error("RUSTCHAT_DB_MAX_CONNECTIONS must be a positive integer")]
    InvalidDbMaxConnections,
    #[error("RUSTCHAT_TRUSTED_PROXY_HOPS must be a non-negative integer")]
    InvalidTrustedProxyHops,
    #[error("RUSTCHAT_GROUP_MEMBER_LIMIT must be between 2 and 500")]
    InvalidGroupMemberLimit,
    #[error("RUSTCHAT_SESSION_IDLE_TIMEOUT_DAYS must be between 1 and 30")]
    InvalidSessionIdleTimeout,
    #[error("authentication rate limits must be between 1 and 100000")]
    InvalidRateLimit,
    #[error(
        "RUSTCHAT_MFA_ENCRYPTION_KEY must be 64 hexadecimal characters (32 random bytes, for example from `openssl rand -hex 32`)"
    )]
    InvalidMfaKey,
    #[error("RUSTCHAT_MFA_ISSUER must be 1 to 64 characters and must not contain a colon")]
    InvalidMfaIssuer,
    #[error(
        "SMTP configuration requires RUSTCHAT_SMTP_HOST, RUSTCHAT_EMAIL_FROM, and RUSTCHAT_PUBLIC_URL"
    )]
    IncompleteEmailConfig,
    #[error("RUSTCHAT_SMTP_PORT must be a valid port number")]
    InvalidSmtpPort,
    #[error("RUSTCHAT_SMTP_USERNAME and RUSTCHAT_SMTP_PASSWORD must be configured together")]
    IncompleteSmtpCredentials,
    #[error(
        "S3-compatible storage configuration requires endpoint, bucket, access key, secret key, and region"
    )]
    IncompleteObjectStorageConfig,
    #[error("RUSTCHAT_S3_ENDPOINT must be an http(s) URL without a path")]
    InvalidObjectStorageEndpoint,
    #[error("RUSTCHAT_ATTACHMENT_MAX_MIB must be between 1 and 100")]
    InvalidAttachmentMax,
    #[error("RUSTCHAT_ATTACHMENT_QUOTA_GIB must be between 1 and 100000")]
    InvalidAttachmentQuota,
}

/// A database URL. It usually embeds a password, so `Debug` is redacted.
#[derive(Clone, PartialEq, Eq)]
pub struct DatabaseUrl(String);

impl DatabaseUrl {
    pub fn from_lookup(lookup: &impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        lookup("DATABASE_URL")
            .filter(|value| !value.is_empty())
            .map(Self)
            .ok_or(ConfigError::MissingDatabaseUrl)
    }

    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(&|key| std::env::var(key).ok())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for DatabaseUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DatabaseUrl(<redacted>)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub bind_addr: SocketAddr,
    pub database_url: DatabaseUrl,
    pub db_max_connections: u32,
    pub registration: RegistrationPolicy,
    pub admin_user_ids: HashSet<UserId>,
    pub allowed_origins: Vec<String>,
    pub cookie_secure: bool,
    pub client_ip: ClientIpSource,
    pub group_member_limit: usize,
    /// Sessions end after this long without activity (and at the 30-day absolute limit).
    pub session_idle_timeout: time::Duration,
    pub rate_limits: RateLimitConfig,
    /// Per-user budgets for general API traffic (per server process).
    pub api_rate_limits: ApiRateLimitConfig,
    pub email: Option<EmailConfig>,
    /// Present when an encryption key is configured, which enables TOTP enrolment.
    pub mfa: Option<MfaConfig>,
    pub object_storage: Option<ObjectStorageConfig>,
    pub attachment_max_file_bytes: i64,
    pub attachment_quota_bytes: i64,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ObjectStorageConfig {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key: String,
    pub secret_key: String,
}

impl fmt::Debug for ObjectStorageConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ObjectStorageConfig")
            .field("endpoint", &self.endpoint)
            .field("region", &self.region)
            .field("bucket", &self.bucket)
            .field("access_key", &"<redacted>")
            .field("secret_key", &"<redacted>")
            .finish()
    }
}

/// The 256-bit key that protects stored TOTP secrets. `Debug` is redacted.
#[derive(Clone, PartialEq, Eq)]
pub struct MfaKey([u8; 32]);

impl MfaKey {
    pub fn parse_hex(raw: &str) -> Result<Self, ConfigError> {
        let raw = raw.trim();
        if raw.len() != 64 || !raw.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(ConfigError::InvalidMfaKey);
        }
        let mut key = [0u8; 32];
        for (slot, pair) in key.iter_mut().zip(raw.as_bytes().chunks(2)) {
            let pair = std::str::from_utf8(pair).map_err(|_| ConfigError::InvalidMfaKey)?;
            *slot = u8::from_str_radix(pair, 16).map_err(|_| ConfigError::InvalidMfaKey)?;
        }
        Ok(Self(key))
    }

    pub fn bytes(&self) -> [u8; 32] {
        self.0
    }
}

impl fmt::Debug for MfaKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MfaKey(<redacted>)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MfaConfig {
    pub key: MfaKey,
    /// Shown in authenticator apps next to the account name.
    pub issuer: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct EmailConfig {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub from: String,
    pub public_url: String,
}

impl fmt::Debug for EmailConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EmailConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username.as_ref().map(|_| "<redacted>"))
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("from", &self.from)
            .field("public_url", &self.public_url)
            .finish()
    }
}

fn is_valid_origin(origin: &str) -> bool {
    let Some(host) = origin
        .strip_prefix("https://")
        .or_else(|| origin.strip_prefix("http://"))
    else {
        return false;
    };
    !host.is_empty() && !host.contains(['/', '?', '#', ' ', '@'])
}

impl Config {
    /// Build configuration from a lookup function (injectable for tests).
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let bind_addr = lookup("RUSTCHAT_BIND")
            .unwrap_or_else(|| DEFAULT_BIND.to_owned())
            .parse()
            .map_err(|_| ConfigError::InvalidBindAddr)?;

        let database_url = DatabaseUrl::from_lookup(&lookup)?;

        let db_max_connections = match lookup("RUSTCHAT_DB_MAX_CONNECTIONS") {
            None => DEFAULT_DB_MAX_CONNECTIONS,
            Some(raw) => raw
                .parse::<u32>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or(ConfigError::InvalidDbMaxConnections)?,
        };

        let registration = match lookup("RUSTCHAT_REGISTRATION").as_deref() {
            None | Some("open") => RegistrationPolicy::Open,
            Some("closed") => RegistrationPolicy::Closed,
            Some(_) => return Err(ConfigError::InvalidRegistration),
        };

        let admin_user_ids = lookup("RUSTCHAT_ADMIN_USER_IDS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| {
                Uuid::parse_str(value)
                    .map(UserId::from_uuid)
                    .map_err(|_| ConfigError::InvalidAdminUserIds)
            })
            .collect::<Result<HashSet<_>, _>>()?;

        let group_member_limit = match lookup("RUSTCHAT_GROUP_MEMBER_LIMIT") {
            None => DEFAULT_GROUP_MEMBER_LIMIT,
            Some(raw) => raw
                .parse::<usize>()
                .ok()
                .filter(|n| (2..=MAX_GROUP_MEMBER_LIMIT).contains(n))
                .ok_or(ConfigError::InvalidGroupMemberLimit)?,
        };

        let session_idle_timeout = match lookup("RUSTCHAT_SESSION_IDLE_TIMEOUT_DAYS") {
            None => time::Duration::days(DEFAULT_SESSION_IDLE_DAYS),
            Some(raw) => time::Duration::days(
                raw.parse::<i64>()
                    .ok()
                    .filter(|n| (1..=MAX_SESSION_IDLE_DAYS).contains(n))
                    .ok_or(ConfigError::InvalidSessionIdleTimeout)?,
            ),
        };

        let default_rate_limits = RateLimitConfig::default();
        let rate_limit = |key: &str, default: u32| -> Result<u32, ConfigError> {
            match lookup(key) {
                None => Ok(default),
                Some(raw) => raw
                    .parse::<u32>()
                    .ok()
                    .filter(|limit| (1..=MAX_RATE_LIMIT).contains(limit))
                    .ok_or(ConfigError::InvalidRateLimit),
            }
        };
        let rate_limits = RateLimitConfig {
            login_per_ip: RateLimitRule {
                max: rate_limit(
                    "RUSTCHAT_LOGIN_PER_IP_LIMIT",
                    default_rate_limits.login_per_ip.max,
                )?,
                window: default_rate_limits.login_per_ip.window,
            },
            login_failures_per_user: RateLimitRule {
                max: rate_limit(
                    "RUSTCHAT_LOGIN_PER_USERNAME_LIMIT",
                    default_rate_limits.login_failures_per_user.max,
                )?,
                window: default_rate_limits.login_failures_per_user.window,
            },
            register_per_ip: RateLimitRule {
                max: rate_limit(
                    "RUSTCHAT_REGISTER_PER_IP_LIMIT",
                    default_rate_limits.register_per_ip.max,
                )?,
                window: default_rate_limits.register_per_ip.window,
            },
        };

        let mfa = match lookup("RUSTCHAT_MFA_ENCRYPTION_KEY").filter(|v| !v.trim().is_empty()) {
            None => None,
            Some(raw) => {
                let issuer = lookup("RUSTCHAT_MFA_ISSUER")
                    .map(|value| value.trim().to_owned())
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| "RustChat".to_owned());
                if issuer.chars().count() > 64 || issuer.contains(':') {
                    return Err(ConfigError::InvalidMfaIssuer);
                }
                Some(MfaConfig {
                    key: MfaKey::parse_hex(&raw)?,
                    issuer,
                })
            }
        };

        let default_api = ApiRateLimitConfig::default();
        let api_rule = |key: &str, default: RateLimitRule| -> Result<RateLimitRule, ConfigError> {
            Ok(RateLimitRule {
                max: rate_limit(key, default.max)?,
                window: default.window,
            })
        };
        let api_rate_limits = ApiRateLimitConfig {
            reads_per_user: api_rule("RUSTCHAT_API_READ_LIMIT", default_api.reads_per_user)?,
            writes_per_user: api_rule("RUSTCHAT_API_WRITE_LIMIT", default_api.writes_per_user)?,
            messages_per_user: api_rule(
                "RUSTCHAT_MESSAGE_SEND_LIMIT",
                default_api.messages_per_user,
            )?,
            creations_per_user: api_rule("RUSTCHAT_CREATE_LIMIT", default_api.creations_per_user)?,
            unauthenticated_per_ip: api_rule(
                "RUSTCHAT_UNAUTHENTICATED_LIMIT",
                default_api.unauthenticated_per_ip,
            )?,
        };

        let allowed_origins: Vec<String> = lookup("RUSTCHAT_ALLOWED_ORIGINS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|origin| !origin.is_empty())
            .map(str::to_owned)
            .collect();
        if allowed_origins.is_empty() {
            return Err(ConfigError::MissingAllowedOrigins);
        }
        if !allowed_origins.iter().all(|o| is_valid_origin(o)) {
            return Err(ConfigError::InvalidAllowedOrigin);
        }

        // Secure by default; `false` is for plain-HTTP local development only.
        let cookie_secure = match lookup("RUSTCHAT_COOKIE_SECURE").as_deref() {
            None | Some("true") => true,
            Some("false") => false,
            Some(_) => return Err(ConfigError::InvalidCookieSecure),
        };

        // 0 (default): use the TCP peer address. N > 0: the server is only
        // reachable through N trusted reverse proxies; use X-Forwarded-For.
        let client_ip = match lookup("RUSTCHAT_TRUSTED_PROXY_HOPS") {
            None => ClientIpSource::Peer,
            Some(raw) => {
                let hops: usize = raw
                    .parse()
                    .map_err(|_| ConfigError::InvalidTrustedProxyHops)?;
                NonZeroUsize::new(hops).map_or(ClientIpSource::Peer, |trusted_hops| {
                    ClientIpSource::ForwardedFor { trusted_hops }
                })
            }
        };

        let smtp_host = lookup("RUSTCHAT_SMTP_HOST").filter(|value| !value.trim().is_empty());
        let email_from = lookup("RUSTCHAT_EMAIL_FROM").filter(|value| !value.trim().is_empty());
        let public_url = lookup("RUSTCHAT_PUBLIC_URL").filter(|value| !value.trim().is_empty());
        let email = if smtp_host.is_none() && email_from.is_none() && public_url.is_none() {
            None
        } else {
            let host = smtp_host.ok_or(ConfigError::IncompleteEmailConfig)?;
            let from = email_from.ok_or(ConfigError::IncompleteEmailConfig)?;
            let public_url = public_url.ok_or(ConfigError::IncompleteEmailConfig)?;
            let port = lookup("RUSTCHAT_SMTP_PORT")
                .unwrap_or_else(|| "465".to_owned())
                .parse::<u16>()
                .ok()
                .filter(|port| *port > 0)
                .ok_or(ConfigError::InvalidSmtpPort)?;
            let username = lookup("RUSTCHAT_SMTP_USERNAME").filter(|value| !value.is_empty());
            let password = lookup("RUSTCHAT_SMTP_PASSWORD").filter(|value| !value.is_empty());
            if username.is_some() != password.is_some() {
                return Err(ConfigError::IncompleteSmtpCredentials);
            }
            Some(EmailConfig {
                host,
                port,
                username,
                password,
                from,
                public_url,
            })
        };

        let s3_endpoint = lookup("RUSTCHAT_S3_ENDPOINT").filter(|value| !value.trim().is_empty());
        let s3_bucket = lookup("RUSTCHAT_S3_BUCKET").filter(|value| !value.trim().is_empty());
        let s3_access_key = lookup("RUSTCHAT_S3_ACCESS_KEY_ID").filter(|value| !value.is_empty());
        let s3_secret_key =
            lookup("RUSTCHAT_S3_SECRET_ACCESS_KEY").filter(|value| !value.is_empty());
        let s3_region = lookup("RUSTCHAT_S3_REGION").filter(|value| !value.trim().is_empty());
        let configured = [
            s3_endpoint.is_some(),
            s3_bucket.is_some(),
            s3_access_key.is_some(),
            s3_secret_key.is_some(),
            s3_region.is_some(),
        ];
        let object_storage = if configured.iter().all(|value| !value) {
            None
        } else if configured.iter().any(|value| !value) {
            return Err(ConfigError::IncompleteObjectStorageConfig);
        } else {
            let endpoint = s3_endpoint.unwrap();
            let valid_scheme = endpoint.starts_with("https://") || endpoint.starts_with("http://");
            let authority = endpoint
                .split_once("://")
                .map(|(_, rest)| rest)
                .unwrap_or_default();
            if !valid_scheme
                || authority.is_empty()
                || authority.contains(['/', '?', '#', '@', ' '])
                || (cookie_secure && endpoint.starts_with("http://"))
            {
                return Err(ConfigError::InvalidObjectStorageEndpoint);
            }
            Some(ObjectStorageConfig {
                endpoint,
                region: s3_region.unwrap(),
                bucket: s3_bucket.unwrap(),
                access_key: s3_access_key.unwrap(),
                secret_key: s3_secret_key.unwrap(),
            })
        };

        let attachment_max_mib = lookup("RUSTCHAT_ATTACHMENT_MAX_MIB")
            .map(|raw| {
                raw.parse::<u64>()
                    .ok()
                    .filter(|n| (1..=MAX_ATTACHMENT_MAX_MIB).contains(n))
                    .ok_or(ConfigError::InvalidAttachmentMax)
            })
            .transpose()?
            .unwrap_or(DEFAULT_ATTACHMENT_MAX_MIB);
        let attachment_quota_gib = lookup("RUSTCHAT_ATTACHMENT_QUOTA_GIB")
            .map(|raw| {
                raw.parse::<u64>()
                    .ok()
                    .filter(|n| (1..=100_000).contains(n))
                    .ok_or(ConfigError::InvalidAttachmentQuota)
            })
            .transpose()?
            .unwrap_or(DEFAULT_ATTACHMENT_QUOTA_GIB);

        Ok(Self {
            bind_addr,
            database_url,
            db_max_connections,
            registration,
            admin_user_ids,
            allowed_origins,
            cookie_secure,
            client_ip,
            group_member_limit,
            session_idle_timeout,
            rate_limits,
            api_rate_limits,
            email,
            mfa,
            object_storage,
            attachment_max_file_bytes: i64::try_from(attachment_max_mib * 1024 * 1024)
                .map_err(|_| ConfigError::InvalidAttachmentMax)?,
            attachment_quota_bytes: i64::try_from(attachment_quota_gib * 1024 * 1024 * 1024)
                .map_err(|_| ConfigError::InvalidAttachmentQuota)?,
        })
    }

    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |key| map.get(key).cloned()
    }

    const BASE: [(&str, &str); 2] = [
        ("DATABASE_URL", "postgres://u:secret@localhost/db"),
        ("RUSTCHAT_ALLOWED_ORIGINS", "https://chat.example.com"),
    ];

    fn with(extra: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let mut pairs = BASE.to_vec();
        pairs.extend_from_slice(extra);
        Config::from_lookup(env(&pairs))
    }

    #[test]
    fn defaults_are_restrictive() {
        let config = with(&[]).unwrap();
        assert_eq!(config.bind_addr, "127.0.0.1:8080".parse().unwrap());
        assert_eq!(config.registration, RegistrationPolicy::Open);
        assert!(config.admin_user_ids.is_empty());
        assert!(config.cookie_secure);
        assert_eq!(config.db_max_connections, 10);
        assert_eq!(config.client_ip, ClientIpSource::Peer);
        assert_eq!(config.group_member_limit, 100);
        assert_eq!(config.rate_limits, RateLimitConfig::default());
        assert!(config.email.is_none());
        assert!(config.object_storage.is_none());
    }

    #[test]
    fn object_storage_is_optional_but_must_be_fully_configured() {
        let configured = with(&[
            ("RUSTCHAT_S3_ENDPOINT", "https://objects.example.test"),
            ("RUSTCHAT_S3_REGION", "garage"),
            ("RUSTCHAT_S3_BUCKET", "rustchat"),
            ("RUSTCHAT_S3_ACCESS_KEY_ID", "key"),
            ("RUSTCHAT_S3_SECRET_ACCESS_KEY", "secret"),
        ])
        .unwrap();
        assert_eq!(configured.object_storage.unwrap().bucket, "rustchat");
        assert_eq!(
            with(&[("RUSTCHAT_S3_BUCKET", "rustchat")]).unwrap_err(),
            ConfigError::IncompleteObjectStorageConfig
        );
    }

    #[test]
    fn smtp_settings_are_optional_complete_and_redacted() {
        let email_config = with(&[
            ("RUSTCHAT_SMTP_HOST", "smtp.example.test"),
            ("RUSTCHAT_SMTP_USERNAME", "mailer"),
            ("RUSTCHAT_SMTP_PASSWORD", "smtp-secret"),
            ("RUSTCHAT_EMAIL_FROM", "RustChat <chat@example.test>"),
            ("RUSTCHAT_PUBLIC_URL", "https://chat.example.test"),
        ])
        .unwrap()
        .email
        .unwrap();
        let debug = format!("{email_config:?}");
        assert!(debug.contains("smtp.example.test"));
        assert!(!debug.contains("smtp-secret"));
        assert!(!debug.contains("mailer"));
        assert_eq!(email_config.port, 465);
        assert_eq!(
            with(&[("RUSTCHAT_SMTP_HOST", "smtp.example.test")]).unwrap_err(),
            ConfigError::IncompleteEmailConfig
        );
    }

    #[test]
    fn trusted_proxy_hops_select_the_client_ip_source() {
        let source =
            |value: &str| with(&[("RUSTCHAT_TRUSTED_PROXY_HOPS", value)]).map(|c| c.client_ip);
        assert_eq!(source("0").unwrap(), ClientIpSource::Peer);
        assert_eq!(
            source("1").unwrap(),
            ClientIpSource::ForwardedFor {
                trusted_hops: NonZeroUsize::new(1).unwrap()
            }
        );
        assert_eq!(
            source("-1").unwrap_err(),
            ConfigError::InvalidTrustedProxyHops
        );
        assert_eq!(
            source("one").unwrap_err(),
            ConfigError::InvalidTrustedProxyHops
        );
    }

    #[test]
    fn reads_overrides() {
        let config = with(&[
            ("RUSTCHAT_BIND", "0.0.0.0:9000"),
            ("RUSTCHAT_REGISTRATION", "closed"),
            ("RUSTCHAT_COOKIE_SECURE", "false"),
            ("RUSTCHAT_DB_MAX_CONNECTIONS", "3"),
            ("RUSTCHAT_GROUP_MEMBER_LIMIT", "64"),
            ("RUSTCHAT_LOGIN_PER_IP_LIMIT", "24"),
            ("RUSTCHAT_LOGIN_PER_USERNAME_LIMIT", "8"),
            ("RUSTCHAT_REGISTER_PER_IP_LIMIT", "4"),
            (
                "RUSTCHAT_ADMIN_USER_IDS",
                "00000000-0000-0000-0000-000000000001",
            ),
            (
                "RUSTCHAT_ALLOWED_ORIGINS",
                "http://localhost:5173, https://a.test",
            ),
        ])
        .unwrap();
        assert_eq!(config.bind_addr, "0.0.0.0:9000".parse().unwrap());
        assert_eq!(config.registration, RegistrationPolicy::Closed);
        assert!(!config.cookie_secure);
        assert_eq!(config.db_max_connections, 3);
        assert_eq!(config.group_member_limit, 64);
        assert_eq!(config.rate_limits.login_per_ip.max, 24);
        assert_eq!(config.rate_limits.login_failures_per_user.max, 8);
        assert_eq!(config.rate_limits.register_per_ip.max, 4);
        assert!(
            config
                .admin_user_ids
                .contains(&UserId::from_uuid(Uuid::from_u128(1)))
        );
        assert_eq!(
            config.allowed_origins,
            ["http://localhost:5173", "https://a.test"]
        );
    }

    #[test]
    fn administrator_ids_must_be_uuids() {
        assert_eq!(
            with(&[("RUSTCHAT_ADMIN_USER_IDS", "not-a-uuid")]).unwrap_err(),
            ConfigError::InvalidAdminUserIds
        );
    }

    #[test]
    fn rejects_invalid_values() {
        let cases: [(&str, &str, ConfigError); 6] = [
            ("RUSTCHAT_BIND", "nope", ConfigError::InvalidBindAddr),
            (
                "RUSTCHAT_REGISTRATION",
                "invite",
                ConfigError::InvalidRegistration,
            ),
            (
                "RUSTCHAT_COOKIE_SECURE",
                "yes",
                ConfigError::InvalidCookieSecure,
            ),
            (
                "RUSTCHAT_DB_MAX_CONNECTIONS",
                "0",
                ConfigError::InvalidDbMaxConnections,
            ),
            (
                "RUSTCHAT_DB_MAX_CONNECTIONS",
                "many",
                ConfigError::InvalidDbMaxConnections,
            ),
            (
                "RUSTCHAT_ALLOWED_ORIGINS",
                "https://a.test/path",
                ConfigError::InvalidAllowedOrigin,
            ),
        ];
        for (key, value, expected) in cases {
            assert_eq!(
                with(&[(key, value)]).unwrap_err(),
                expected,
                "{key}={value}"
            );
        }
        for bad in ["a.test", "ftp://a.test", "https://", "https://a.test/", "*"] {
            assert_eq!(
                with(&[("RUSTCHAT_ALLOWED_ORIGINS", bad)]).unwrap_err(),
                ConfigError::InvalidAllowedOrigin,
                "{bad}"
            );
        }
    }

    #[test]
    fn requires_database_url_and_allowed_origins() {
        let no_db = Config::from_lookup(env(&[("RUSTCHAT_ALLOWED_ORIGINS", "https://a.test")]));
        assert_eq!(no_db.unwrap_err(), ConfigError::MissingDatabaseUrl);
        let no_origins = Config::from_lookup(env(&[("DATABASE_URL", "postgres://x")]));
        assert_eq!(no_origins.unwrap_err(), ConfigError::MissingAllowedOrigins);
        let blank = with(&[("RUSTCHAT_ALLOWED_ORIGINS", " , ")]);
        assert_eq!(blank.unwrap_err(), ConfigError::MissingAllowedOrigins);
    }

    #[test]
    fn api_rate_limits_default_and_are_configurable_within_bounds() {
        let defaults = with(&[]).unwrap().api_rate_limits;
        assert_eq!(defaults, ApiRateLimitConfig::default());
        let tuned = with(&[
            ("RUSTCHAT_API_READ_LIMIT", "900"),
            ("RUSTCHAT_API_WRITE_LIMIT", "200"),
            ("RUSTCHAT_MESSAGE_SEND_LIMIT", "90"),
            ("RUSTCHAT_CREATE_LIMIT", "50"),
            ("RUSTCHAT_UNAUTHENTICATED_LIMIT", "10"),
        ])
        .unwrap()
        .api_rate_limits;
        assert_eq!(tuned.reads_per_user.max, 900);
        assert_eq!(tuned.writes_per_user.max, 200);
        assert_eq!(tuned.messages_per_user.max, 90);
        assert_eq!(tuned.creations_per_user.max, 50);
        assert_eq!(tuned.unauthenticated_per_ip.max, 10);
        // Windows are fixed, as for the authentication limits.
        assert_eq!(
            tuned.messages_per_user.window,
            defaults.messages_per_user.window
        );
        for key in [
            "RUSTCHAT_API_READ_LIMIT",
            "RUSTCHAT_API_WRITE_LIMIT",
            "RUSTCHAT_MESSAGE_SEND_LIMIT",
            "RUSTCHAT_CREATE_LIMIT",
            "RUSTCHAT_UNAUTHENTICATED_LIMIT",
        ] {
            for value in ["0", "100001", "lots"] {
                assert_eq!(
                    with(&[(key, value)]).unwrap_err(),
                    ConfigError::InvalidRateLimit,
                    "{key}={value}"
                );
            }
        }
    }

    #[test]
    fn mfa_is_off_without_a_key_and_validates_the_key_and_issuer() {
        assert_eq!(with(&[]).unwrap().mfa, None);
        assert_eq!(
            with(&[("RUSTCHAT_MFA_ENCRYPTION_KEY", "  ")]).unwrap().mfa,
            None
        );

        let key = "00".repeat(31) + "ff";
        let config = with(&[("RUSTCHAT_MFA_ENCRYPTION_KEY", &key)]).unwrap();
        let mfa = config.mfa.expect("configured");
        assert_eq!(mfa.issuer, "RustChat");
        assert_eq!(mfa.key.bytes()[31], 0xff);
        assert_eq!(mfa.key.bytes()[0], 0);
        let named = with(&[
            ("RUSTCHAT_MFA_ENCRYPTION_KEY", &key),
            ("RUSTCHAT_MFA_ISSUER", "Acme Chat"),
        ])
        .unwrap();
        assert_eq!(named.mfa.unwrap().issuer, "Acme Chat");

        for bad in ["abc", &"zz".repeat(32), &"00".repeat(31), &"00".repeat(33)] {
            assert_eq!(
                with(&[("RUSTCHAT_MFA_ENCRYPTION_KEY", bad)]).unwrap_err(),
                ConfigError::InvalidMfaKey,
                "{bad}"
            );
        }
        for issuer in ["a:b", &"x".repeat(65)] {
            assert_eq!(
                with(&[
                    ("RUSTCHAT_MFA_ENCRYPTION_KEY", &key),
                    ("RUSTCHAT_MFA_ISSUER", issuer)
                ])
                .unwrap_err(),
                ConfigError::InvalidMfaIssuer
            );
        }
    }

    #[test]
    fn the_mfa_key_never_appears_in_debug_output() {
        let key = "ab".repeat(32);
        let config = with(&[("RUSTCHAT_MFA_ENCRYPTION_KEY", &key)]).unwrap();
        let printed = format!("{config:?}");
        assert!(!printed.contains(&key), "{printed}");
        assert!(printed.contains("MfaKey(<redacted>)"));
    }

    #[test]
    fn session_idle_timeout_defaults_and_is_bounded() {
        assert_eq!(
            with(&[]).unwrap().session_idle_timeout,
            time::Duration::days(7)
        );
        assert_eq!(
            with(&[("RUSTCHAT_SESSION_IDLE_TIMEOUT_DAYS", "30")])
                .unwrap()
                .session_idle_timeout,
            time::Duration::days(30)
        );
        for value in ["0", "31", "-1", "week"] {
            assert_eq!(
                with(&[("RUSTCHAT_SESSION_IDLE_TIMEOUT_DAYS", value)]).unwrap_err(),
                ConfigError::InvalidSessionIdleTimeout
            );
        }
    }

    #[test]
    fn group_member_limit_is_bounded() {
        for value in ["1", "501", "many"] {
            assert_eq!(
                with(&[("RUSTCHAT_GROUP_MEMBER_LIMIT", value)]).unwrap_err(),
                ConfigError::InvalidGroupMemberLimit
            );
        }
        assert_eq!(
            with(&[("RUSTCHAT_GROUP_MEMBER_LIMIT", "2")])
                .unwrap()
                .group_member_limit,
            2
        );
        assert_eq!(
            with(&[("RUSTCHAT_GROUP_MEMBER_LIMIT", "500")])
                .unwrap()
                .group_member_limit,
            500
        );
    }

    #[test]
    fn authentication_rate_limits_are_bounded() {
        for value in ["0", "100001", "invalid"] {
            assert_eq!(
                with(&[("RUSTCHAT_LOGIN_PER_IP_LIMIT", value)]).unwrap_err(),
                ConfigError::InvalidRateLimit
            );
        }
        assert_eq!(
            with(&[("RUSTCHAT_REGISTER_PER_IP_LIMIT", "1")])
                .unwrap()
                .rate_limits
                .register_per_ip
                .max,
            1
        );
    }

    #[test]
    fn debug_output_never_contains_the_database_password() {
        let config = with(&[]).unwrap();
        let printed = format!("{config:?}");
        assert!(!printed.contains("secret"), "{printed}");
    }
}
