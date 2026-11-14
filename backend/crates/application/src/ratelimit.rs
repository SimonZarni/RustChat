//! Rate-limit policy for authentication endpoints.

use std::net::{IpAddr, Ipv6Addr};

use time::Duration;

use crate::ports::RateLimitRule;

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

    #[test]
    fn key_namespaces_do_not_collide() {
        assert_ne!(login_ip_key(ip("1.2.3.4")), register_ip_key(ip("1.2.3.4")));
    }
}
