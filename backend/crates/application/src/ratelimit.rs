//! Rate-limit policy: authentication endpoints and general API traffic.

use std::{
    net::{IpAddr, Ipv6Addr},
    sync::Arc,
};

use rustchat_domain::identity::UserId;
use time::{Duration, OffsetDateTime};

use crate::{
    error::AppError,
    ports::{Clock, RateLimitDecision, RateLimitRule, RateLimiter},
};

/// Longest username (in characters) used to build a limiter key. Longer
/// inputs are truncated so key size, and therefore memory, stays bounded.
const MAX_KEY_USERNAME_CHARS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitConfig {
    /// Login attempts of any outcome per client network. Bounds password
    /// guessing across many usernames and floods of successful logins.
    pub login_per_ip: RateLimitRule,
    /// Failed logins per username across all clients. Bounds distributed
    /// guessing against one account. The trade-off: an attacker can keep a
    /// victim's logins blocked for one window by failing on purpose.
    pub login_failures_per_user: RateLimitRule,
    /// Registration attempts of any outcome per client network.
    pub register_per_ip: RateLimitRule,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            login_per_ip: RateLimitRule {
                max: 30,
                window: Duration::minutes(15),
            },
            login_failures_per_user: RateLimitRule {
                max: 10,
                window: Duration::minutes(15),
            },
            register_per_ip: RateLimitRule {
                max: 5,
                window: Duration::hours(1),
            },
        }
    }
}

/// Classes of authenticated API traffic. Each class has its own per-user budget
/// so a burst of one kind (for example reads while scrolling history) cannot
/// starve another (sending messages).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ApiOperation {
    /// `GET`, `HEAD` and `OPTIONS` requests.
    Read,
    /// State-changing requests not covered by a more specific class.
    Write,
    /// Sending a message.
    SendMessage,
    /// Creating a conversation, community, channel, category, role or invite.
    Create,
}

impl ApiOperation {
    fn label(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::SendMessage => "message",
            Self::Create => "create",
        }
    }
}

/// Budgets for general API traffic. Authentication endpoints have their own
/// stricter limits in [`RateLimitConfig`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiRateLimitConfig {
    pub reads_per_user: RateLimitRule,
    pub writes_per_user: RateLimitRule,
    pub messages_per_user: RateLimitRule,
    pub creations_per_user: RateLimitRule,
    /// Requests per client network that fail authentication. Bounds floods of
    /// unauthenticated requests; legitimate clients rarely send any.
    pub unauthenticated_per_ip: RateLimitRule,
}

impl Default for ApiRateLimitConfig {
    fn default() -> Self {
        let per_minute = |max| RateLimitRule {
            max,
            window: Duration::minutes(1),
        };
        Self {
            reads_per_user: per_minute(600),
            writes_per_user: per_minute(120),
            messages_per_user: per_minute(60),
            creations_per_user: RateLimitRule {
                max: 30,
                window: Duration::hours(1),
            },
            unauthenticated_per_ip: per_minute(120),
        }
    }
}

/// Enforces [`ApiRateLimitConfig`] with a [`RateLimiter`]. Every counted
/// request spends budget whether or not it later succeeds, so retry storms
/// cannot bypass the limit.
pub struct ApiRateLimiter {
    limiter: Arc<dyn RateLimiter>,
    clock: Arc<dyn Clock>,
    config: ApiRateLimitConfig,
}

impl ApiRateLimiter {
    pub fn new(
        limiter: Arc<dyn RateLimiter>,
        clock: Arc<dyn Clock>,
        config: ApiRateLimitConfig,
    ) -> Self {
        Self {
            limiter,
            clock,
            config,
        }
    }

    /// Spend one request of `operation` for `user`, or fail with `RateLimited`.
    pub async fn enforce_user(
        &self,
        user: UserId,
        operation: ApiOperation,
    ) -> Result<(), AppError> {
        let rule = match operation {
            ApiOperation::Read => self.config.reads_per_user,
            ApiOperation::Write => self.config.writes_per_user,
            ApiOperation::SendMessage => self.config.messages_per_user,
            ApiOperation::Create => self.config.creations_per_user,
        };
        let key = format!("api:{}:{}", operation.label(), user.as_uuid());
        self.spend(&key, rule).await
    }

    /// Spend one failed-authentication request for a client network.
    pub async fn enforce_unauthenticated(&self, client_ip: IpAddr) -> Result<(), AppError> {
        let key = format!("api:unauthenticated:{}", ip_bucket(client_ip));
        self.spend(&key, self.config.unauthenticated_per_ip).await
    }

    async fn spend(&self, key: &str, rule: RateLimitRule) -> Result<(), AppError> {
        let now: OffsetDateTime = self.clock.now();
        match self.limiter.check(key, rule, now).await? {
            RateLimitDecision::Allowed => self.limiter.record(key, rule, now).await,
            RateLimitDecision::Limited { retry_after } => Err(AppError::RateLimited {
                // Round up so a client that waits this long is not refused again.
                retry_after_secs: u64::try_from(retry_after.whole_seconds())
                    .unwrap_or(0)
                    .saturating_add(u64::from(retry_after.subsec_nanoseconds() > 0))
                    .max(1),
            }),
        }
    }
}

/// The network a client is counted under. IPv6 clients are grouped by /64,
/// since a single subscriber can use any address in their prefix.
pub fn ip_bucket(ip: IpAddr) -> String {
    match ip.to_canonical() {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => {
            let mut octets = v6.octets();
            octets[8..].fill(0);
            format!("{}/64", Ipv6Addr::from(octets))
        }
    }
}

pub fn login_ip_key(ip: IpAddr) -> String {
    format!("login:ip:{}", ip_bucket(ip))
}

/// Keyed by the submitted name, case-folded, whether or not an account
/// exists, so limiting never reveals which usernames are registered.
pub fn login_user_key(username: &str) -> String {
    let name: String = username
        .chars()
        .take(MAX_KEY_USERNAME_CHARS)
        .collect::<String>()
        .to_lowercase();
    format!("login:user:{name}")
}

pub fn register_ip_key(ip: IpAddr) -> String {
    format!("register:ip:{}", ip_bucket(ip))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn ipv4_is_counted_per_address() {
        assert_eq!(ip_bucket(ip("203.0.113.7")), "203.0.113.7");
        assert_ne!(ip_bucket(ip("203.0.113.7")), ip_bucket(ip("203.0.113.8")));
    }

    #[test]
    fn ipv6_is_counted_per_64_prefix() {
        let a = ip_bucket(ip("2001:db8:1:2:aaaa:bbbb:cccc:dddd"));
        let b = ip_bucket(ip("2001:db8:1:2::1"));
        let other = ip_bucket(ip("2001:db8:1:3::1"));
        assert_eq!(a, b);
        assert_ne!(a, other);
        assert_eq!(a, "2001:db8:1:2::/64");
    }

    #[test]
    fn ipv4_mapped_ipv6_shares_the_ipv4_bucket() {
        assert_eq!(
            ip_bucket(ip("::ffff:203.0.113.7")),
            ip_bucket(ip("203.0.113.7"))
        );
    }

    #[test]
    fn username_keys_are_case_folded_and_bounded() {
        assert_eq!(login_user_key("Alice"), login_user_key("aLiCe"));
        let long = login_user_key(&"x".repeat(10_000));
        assert!(long.len() < 100, "key must stay small: {}", long.len());
    }

    use crate::testing::{FakeClock, FakeLimiter};

    fn api_limiter(config: ApiRateLimitConfig) -> (ApiRateLimiter, Arc<FakeClock>) {
        let clock = Arc::new(FakeClock::new(OffsetDateTime::UNIX_EPOCH));
        let limiter = ApiRateLimiter::new(Arc::new(FakeLimiter::default()), clock.clone(), config);
        (limiter, clock)
    }

    fn tight(max: u32) -> ApiRateLimitConfig {
        let rule = RateLimitRule {
            max,
            window: Duration::minutes(1),
        };
        ApiRateLimitConfig {
            reads_per_user: rule,
            writes_per_user: rule,
            messages_per_user: rule,
            creations_per_user: rule,
            unauthenticated_per_ip: rule,
        }
    }

    #[tokio::test]
    async fn a_class_budget_is_spent_per_user_and_does_not_affect_other_classes_or_users() {
        let (limiter, _) = api_limiter(tight(2));
        let (alice, bob) = (UserId::new(), UserId::new());

        for _ in 0..2 {
            limiter
                .enforce_user(alice, ApiOperation::SendMessage)
                .await
                .unwrap();
        }
        assert!(matches!(
            limiter.enforce_user(alice, ApiOperation::SendMessage).await,
            Err(AppError::RateLimited { retry_after_secs }) if retry_after_secs >= 1
        ));
        // Other classes and other users are unaffected.
        limiter
            .enforce_user(alice, ApiOperation::Read)
            .await
            .unwrap();
        limiter
            .enforce_user(alice, ApiOperation::Create)
            .await
            .unwrap();
        limiter
            .enforce_user(bob, ApiOperation::SendMessage)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn unauthenticated_requests_are_counted_per_network() {
        let (limiter, _) = api_limiter(tight(1));
        limiter
            .enforce_unauthenticated(ip("203.0.113.7"))
            .await
            .unwrap();
        assert!(matches!(
            limiter.enforce_unauthenticated(ip("203.0.113.7")).await,
            Err(AppError::RateLimited { .. })
        ));
        limiter
            .enforce_unauthenticated(ip("203.0.113.8"))
            .await
            .unwrap();
    }

    #[test]
    fn defaults_are_generous_for_people_and_strict_for_creation() {
        let config = ApiRateLimitConfig::default();
        assert!(config.reads_per_user.max > config.writes_per_user.max);
        assert!(config.writes_per_user.max > config.messages_per_user.max);
        assert!(config.creations_per_user.window > config.messages_per_user.window);
    }

    #[test]
    fn key_namespaces_do_not_collide() {
        assert_ne!(login_ip_key(ip("1.2.3.4")), register_ip_key(ip("1.2.3.4")));
    }
}
