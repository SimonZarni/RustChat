//! Runtime configuration, read from environment variables.

use std::net::SocketAddr;

const DEFAULT_BIND: &str = "127.0.0.1:8080";
const BIND_VAR: &str = "RUSTCHAT_BIND";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub bind_addr: SocketAddr,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("{BIND_VAR} is not a valid socket address")]
    InvalidBindAddr,
}

impl Config {
    /// Build configuration from a lookup function (injectable for tests).
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let raw = lookup(BIND_VAR).unwrap_or_else(|| DEFAULT_BIND.to_owned());
        let bind_addr = raw.parse().map_err(|_| ConfigError::InvalidBindAddr)?;
        Ok(Self { bind_addr })
    }

    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_loopback() {
        let config = Config::from_lookup(|_| None).unwrap();
        assert_eq!(config.bind_addr, "127.0.0.1:8080".parse().unwrap());
    }

    #[test]
    fn reads_bind_address_override() {
        let config = Config::from_lookup(|_| Some("0.0.0.0:9000".into())).unwrap();
        assert_eq!(config.bind_addr, "0.0.0.0:9000".parse().unwrap());
    }

    #[test]
    fn rejects_invalid_bind_address() {
        let err = Config::from_lookup(|_| Some("not-an-address".into())).unwrap_err();
        assert_eq!(err, ConfigError::InvalidBindAddr);
    }
}
