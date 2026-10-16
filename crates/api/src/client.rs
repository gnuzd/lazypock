//! Who a request came from.
//!
//! One function, because getting it wrong in one place is as bad as getting it
//! wrong everywhere: an address that can be chosen by the caller makes a rate
//! limit optional and a request log fiction.

use std::net::IpAddr;

use axum::http::HeaderMap;
use lazypock_core::config::Config;

/// The address a request came from.
///
/// * With no configured proxies, **no header is read at all**. `X-Forwarded-For`
///   is set by the client when nothing is in front of the server, so believing it
///   by default is believing whatever the caller wrote.
/// * With the connection coming from a trusted proxy, its header means something.
///   The chain is read **right to left** and the first entry that is not itself a
///   trusted proxy is the client. `X-Forwarded-For` is append-only, so everything
///   to the left of that point was supplied by the client and means nothing — which
///   is why reading it left to right, as this used to, let a caller name themselves.
pub fn address(config: &Config, peer: Option<IpAddr>, headers: &HeaderMap) -> Option<IpAddr> {
    let peer = peer?;

    if !config.trusts(peer) {
        return Some(peer);
    }

    for candidate in forwarded(headers) {
        if !config.trusts(candidate) {
            return Some(candidate);
        }
    }

    // Every entry was a proxy, or the header was absent or unreadable. The peer is
    // the best answer left, and it is a real one.
    Some(peer)
}

/// The forwarding chain, right to left.
///
/// Entries that are not addresses are dropped rather than failing: a proxy that
/// wrote something odd should not cost the request its address, and anything to
/// the left of the first real entry is the client's own text anyway.
fn forwarded(headers: &HeaderMap) -> Vec<IpAddr> {
    let Some(raw) = headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("x-real-ip"))
        .and_then(|value| value.to_str().ok())
    else {
        return Vec::new();
    };

    raw.split(',')
        .rev()
        .filter_map(|entry| entry.trim().parse::<IpAddr>().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header::HeaderValue;
    use lazypock_core::config::Config;

    /// A config with this trusted-proxy list, and nothing else set.
    fn config(trusted: &str) -> Config {
        Config::from_source(|key| match key {
            "LAZYPOCK_DATABASE_URL" => Some("postgres://user:pw@localhost:5432/db".to_string()),
            "LAZYPOCK_TRUSTED_PROXIES" => Some(trusted.to_string()),
            _ => None,
        })
        .expect("a valid configuration")
    }

    fn ip(raw: &str) -> IpAddr {
        raw.parse().expect("a test address")
    }

    fn headers(xff: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_str(xff).expect("a header value"),
        );
        headers
    }

    #[test]
    fn without_a_trusted_proxy_the_header_is_ignored() {
        let config = config("");
        let headers = headers("1.2.3.4");
        assert_eq!(
            address(&config, Some(ip("203.0.113.9")), &headers),
            Some(ip("203.0.113.9"))
        );
    }

    /// The whole point: a caller cannot name themselves by prepending an entry.
    /// A right-to-left read stops at the first address the proxy did not write.
    #[test]
    fn a_client_supplied_prefix_is_ignored() {
        let config = config("10.0.0.1");
        let headers = headers("9.9.9.9, 1.2.3.4");
        assert_eq!(
            address(&config, Some(ip("10.0.0.1")), &headers),
            Some(ip("1.2.3.4")),
            "the leftmost entry is the client's own text and must not win"
        );
    }

    #[test]
    fn a_trusted_proxys_header_names_the_client() {
        let config = config("10.0.0.1");
        let headers = headers("1.2.3.4");
        assert_eq!(
            address(&config, Some(ip("10.0.0.1")), &headers),
            Some(ip("1.2.3.4"))
        );
    }

    #[test]
    fn a_chain_of_trusted_proxies_is_walked_to_the_first_real_client() {
        let config = config("10.0.0.1, 10.0.0.2");
        let headers = headers("1.2.3.4, 10.0.0.2");
        assert_eq!(
            address(&config, Some(ip("10.0.0.1")), &headers),
            Some(ip("1.2.3.4"))
        );
    }

    #[test]
    fn an_unreadable_header_falls_back_to_the_peer() {
        let config = config("10.0.0.1");
        let headers = headers("not-an-address");
        assert_eq!(
            address(&config, Some(ip("10.0.0.1")), &headers),
            Some(ip("10.0.0.1"))
        );

        let empty = HeaderMap::new();
        assert_eq!(
            address(&config, Some(ip("10.0.0.1")), &empty),
            Some(ip("10.0.0.1"))
        );
    }

    /// An in-process request has no peer at all, and nothing remote to believe.
    #[test]
    fn no_peer_means_no_address() {
        let config = config("10.0.0.1");
        assert_eq!(address(&config, None, &headers("1.2.3.4")), None);
    }

    #[test]
    fn x_real_ip_is_read_when_there_is_no_forwarded_for() {
        let config = config("10.0.0.1");
        let mut headers = HeaderMap::new();
        headers.insert("x-real-ip", HeaderValue::from_static("1.2.3.4"));
        assert_eq!(
            address(&config, Some(ip("10.0.0.1")), &headers),
            Some(ip("1.2.3.4"))
        );
    }
}
