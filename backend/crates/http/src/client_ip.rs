//! Determining the client network for rate limiting.
//!
//! The TCP peer address is authoritative unless the operator states that the
//! server sits behind a known number of trusted reverse proxies. Forwarding
//! headers are otherwise ignored, so a client cannot choose its own identity.

use std::{
    net::{IpAddr, SocketAddr},
    num::NonZeroUsize,
};

use axum::{
    extract::{ConnectInfo, FromRequestParts},
    http::{header::HeaderName, request::Parts},
};
use rustchat_application::error::AppError;

use crate::{ApiError, AppState};

const X_FORWARDED_FOR: HeaderName = HeaderName::from_static("x-forwarded-for");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientIpSource {
    /// Use the TCP peer address. Correct when clients connect directly.
    Peer,
    /// Take the address from `X-Forwarded-For`, counting `trusted_hops`
    /// entries from the right. Each trusted proxy appends the address it saw,
    /// so with one proxy the last entry is the client. Only safe when every
    /// request reaches the server through those proxies.
    ForwardedFor { trusted_hops: NonZeroUsize },
}

/// Pure resolution logic. Falls back to the peer address if the header is
/// absent, too short or unparsable; behind a proxy that lumps clients
/// together, which fails safe (stricter) rather than letting a client pick.
pub fn resolve(source: ClientIpSource, forwarded_for: &[&str], peer: IpAddr) -> IpAddr {
    match source {
        ClientIpSource::Peer => peer,
        ClientIpSource::ForwardedFor { trusted_hops } => {
            let entries: Vec<&str> = forwarded_for
                .iter()
                .flat_map(|value| value.split(','))
                .map(str::trim)
                .collect();
            entries
                .len()
                .checked_sub(trusted_hops.get())
                .and_then(|index| entries.get(index))
                .and_then(|entry| entry.parse::<IpAddr>().ok())
                .unwrap_or(peer)
        }
    }
}

/// The resolved client address of the current request.
pub struct ClientIp(pub IpAddr);

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(addr)| addr.ip())
            // Missing connection info is a server wiring error, never a
            // reason to skip rate limiting.
            .ok_or_else(|| AppError::internal("connection info missing from request"))?;
        let forwarded: Vec<&str> = parts
            .headers
            .get_all(X_FORWARDED_FOR)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .collect();
        Ok(Self(resolve(state.config.client_ip, &forwarded, peer)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn hops(n: usize) -> ClientIpSource {
        ClientIpSource::ForwardedFor {
            trusted_hops: NonZeroUsize::new(n).unwrap(),
        }
    }

    const PEER: &str = "10.0.0.1";

    #[test]
    fn peer_mode_ignores_forwarding_headers_entirely() {
        let got = resolve(ClientIpSource::Peer, &["6.6.6.6"], ip(PEER));
        assert_eq!(got, ip(PEER));
    }

    #[test]
    fn one_trusted_proxy_uses_the_last_entry_not_a_spoofed_first_one() {
        // The client sent "6.6.6.6"; the proxy appended the address it saw.
        let got = resolve(hops(1), &["6.6.6.6, 203.0.113.9"], ip(PEER));
        assert_eq!(got, ip("203.0.113.9"));
    }

    #[test]
    fn two_trusted_proxies_skip_the_inner_proxy_entry() {
        let got = resolve(hops(2), &["6.6.6.6, 203.0.113.9, 10.0.0.7"], ip(PEER));
        assert_eq!(got, ip("203.0.113.9"));
    }

    #[test]
    fn multiple_header_lines_are_treated_as_one_list() {
        let got = resolve(hops(1), &["6.6.6.6", "203.0.113.9"], ip(PEER));
        assert_eq!(got, ip("203.0.113.9"));
    }

    #[test]
    fn missing_short_or_garbage_headers_fall_back_to_the_peer() {
        assert_eq!(resolve(hops(1), &[], ip(PEER)), ip(PEER));
        assert_eq!(resolve(hops(2), &["203.0.113.9"], ip(PEER)), ip(PEER));
        assert_eq!(resolve(hops(1), &["not-an-ip"], ip(PEER)), ip(PEER));
        assert_eq!(resolve(hops(1), &["1.2.3.4,"], ip(PEER)), ip(PEER));
    }

    #[test]
    fn ipv6_entries_are_parsed() {
        let got = resolve(hops(1), &["2001:db8::5"], ip(PEER));
        assert_eq!(got, ip("2001:db8::5"));
    }
}
