//! Runtime configuration, read from environment variables.

use std::{collections::HashSet, fmt, net::SocketAddr, num::NonZeroUsize};

use rustchat_application::auth::RegistrationPolicy;
use rustchat_application::{ports::RateLimitRule, ratelimit::RateLimitConfig};
use rustchat_domain::identity::UserId;
use rustchat_http::ClientIpSource;
use uuid::Uuid;

const DEFAULT_BIND: &str = "127.0.0.1:8080";
const DEFAULT_DB_MAX_CONNECTIONS: u32 = 10;
const DEFAULT_GROUP_MEMBER_LIMIT: usize = 100;
const MAX_GROUP_MEMBER_LIMIT: usize = 500;
const MAX_RATE_LIMIT: u32 = 100_000;

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
    #[error("authentication rate limits must be between 1 and 100000")]
    InvalidRateLimit,
    #[error(
        "SMTP configuration requires RUSTCHAT_SMTP_HOST, RUSTCHAT_EMAIL_FROM, and RUSTCHAT_PUBLIC_URL"
    )]
    IncompleteEmailConfig,
    #[error("RUSTCHAT_SMTP_PORT must be a valid port number")]
    InvalidSmtpPort,
    #[error("RUSTCHAT_SMTP_USERNAME and RUSTCHAT_SMTP_PASSWORD must be configured together")]
    IncompleteSmtpCredentials,
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
    pub rate_limits: RateLimitConfig,
    pub email: Option<EmailConfig>,
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
            rate_limits,
            email,
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
