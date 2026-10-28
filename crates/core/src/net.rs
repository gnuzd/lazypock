//! Which peers are allowed to speak for a client.
//!
//! A forwarding header is set by the client when there is no proxy in front, so
//! believing one unconditionally means believing whatever the caller wrote. The
//! only safe rule is: read it, but only when the connection itself came from a
//! proxy you named.
//!
//! This is the whole of that rule's data — a list of addresses and CIDR blocks,
//! parsed from `LAZYPOCK_TRUSTED_PROXIES`.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// One entry: a single address, or a CIDR block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrustedNet {
    base: IpAddr,
    /// How many leading bits are significant.
    prefix: u8,
}

impl TrustedNet {
    /// Parse `10.0.0.1`, `10.0.0.0/8`, `::1` or `fd00::/8`.
    pub fn parse(raw: &str) -> Result<Self, String> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err("an entry is empty".to_string());
        }

        let (address, prefix) = match raw.split_once('/') {
            Some((address, prefix)) => (address, Some(prefix)),
            None => (raw, None),
        };

        let base: IpAddr = address
            .trim()
            .parse()
            .map_err(|_| format!("{address:?} is not an IP address"))?;

        let bits = if base.is_ipv4() { 32u8 } else { 128u8 };
        let prefix = match prefix {
            None => bits,
            Some(prefix) => {
                let parsed: u8 = prefix
                    .trim()
                    .parse()
                    .map_err(|_| format!("{prefix:?} is not a prefix length"))?;
                if parsed > bits {
                    return Err(format!(
                        "a prefix of {parsed} is longer than {bits} bits, the width of {address:?}"
                    ));
                }
                parsed
            }
        };

        Ok(Self { base, prefix })
    }

    /// Whether this entry covers `address`.
    ///
    /// Both sides are compared as 128-bit values, with IPv4 mapped into the
    /// `::ffff:0:0/96` block. An IPv4 rule and the mapped spelling of the same
    /// address then agree — which is what a proxy on a dual-stack socket reports —
    /// without a mapped rule losing the space its prefix was written in:
    /// `::ffff:10.0.0.0/104` is ten-oh-oh-oh/8.
    pub fn contains(&self, address: IpAddr) -> bool {
        let mask = v6_mask(self.prefix_in_v6());
        (as_v6(self.base) & mask) == (as_v6(address) & mask)
    }

    /// The prefix counted from the start of the 128-bit form.
    ///
    /// An IPv4 prefix counts from bit 96, because that is where its address
    /// starts once mapped.
    fn prefix_in_v6(&self) -> u8 {
        match self.base {
            IpAddr::V4(_) => 96 + self.prefix,
            IpAddr::V6(_) => self.prefix,
        }
    }
}

/// An address as 128 bits, with IPv4 mapped into the block above.
fn as_v6(address: IpAddr) -> u128 {
    match address {
        IpAddr::V6(v6) => u128::from(v6),
        IpAddr::V4(v4) => u128::from(v4.to_ipv6_mapped()),
    }
}

/// The mask for a v6 prefix. `prefix` is at most 128, checked at parse time.
fn v6_mask(prefix: u8) -> u128 {
    if prefix == 0 {
        0
    } else {
        u128::MAX << (128 - u32::from(prefix))
    }
}

/// A list of trusted peers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustedProxies {
    nets: Vec<TrustedNet>,
}

impl TrustedProxies {
    /// Parse a comma-separated list. An empty or blank value means none.
    pub fn parse(raw: &str) -> Result<Self, String> {
        let mut nets = Vec::new();
        for entry in raw.split(',').map(str::trim).filter(|e| !e.is_empty()) {
            nets.push(TrustedNet::parse(entry)?);
        }
        Ok(Self { nets })
    }

    /// Whether any entry covers `address`.
    pub fn contains(&self, address: IpAddr) -> bool {
        self.nets.iter().any(|net| net.contains(address))
    }

    /// Whether any peer is trusted at all.
    ///
    /// When this is false the socket address is the only source of truth, and no
    /// header is read — which is the safe default for a deployment that has not
    /// said what its proxy is.
    pub fn is_empty(&self) -> bool {
        self.nets.is_empty()
    }

    /// How many entries, for a log line.
    pub fn len(&self) -> usize {
        self.nets.len()
    }
}

/// One entry in a host allowlist: a host, or a host and its subdomains, on one port.
#[derive(Debug, Clone, PartialEq, Eq)]
struct HostPattern {
    /// The host, lowercased and without a trailing root dot.
    host: String,
    /// `*.example.com` — any subdomain, but not `example.com` itself.
    ///
    /// Deliberately not the plus side: a wildcard that also matched the apex would
    /// let `*.example.com` allow the zone's own mail or admin host, which is rarely
    /// what someone writing it meant.
    subdomains: bool,
    /// The only port this entry grants. HTTPS unless the entry names another.
    ///
    /// Host-only rather than "any port on that host": an allowlisted webhook host
    /// would otherwise also be reachable on its SSH, Redis or SMTP port, which is a
    /// capability nobody writing `example.com` meant to grant.
    port: u16,
}

impl HostPattern {
    fn parse(raw: &str) -> Result<Self, String> {
        let raw = raw.trim().trim_end_matches('.').to_ascii_lowercase();
        if raw.is_empty() {
            return Err("an entry is empty".to_string());
        }
        if raw.contains("://") || raw.contains('/') || raw.contains('@') {
            return Err(format!(
                "{raw:?} is not a host: give a hostname, without a scheme, a path or credentials"
            ));
        }

        // A bare IPv6 literal is refused before the port split, because its own
        // colons would look like one.
        if raw.parse::<IpAddr>().is_ok() || raw.starts_with('[') {
            return Err(format!(
                "{raw:?} is an IP address; name the host, so that the address it resolves to is \
                 checked on every call"
            ));
        }

        let (host, port) = match raw.rsplit_once(':') {
            Some((host, port)) => {
                let port: u16 = port
                    .trim()
                    .parse()
                    .map_err(|_| format!("{port:?} is not a port number"))?;
                if port == 0 {
                    return Err("a port of 0 is not a port".to_string());
                }
                (host, port)
            }
            None => (raw.as_str(), 443),
        };
        if host.parse::<IpAddr>().is_ok() {
            return Err(format!(
                "{host:?} is an IP address; name the host, so that the address it resolves to is \
                 checked on every call"
            ));
        }

        let (wildcard, host) = match host.strip_prefix("*.") {
            Some(rest) => (true, rest),
            None => (false, host),
        };
        if host.is_empty() || host.starts_with('.') || host.ends_with('.') || host.contains('*') {
            return Err(format!("{raw:?} is not a hostname"));
        }
        // A label is letters, digits, hyphen and underscore (`_spf` exists in the
        // wild), and cannot start or end with a hyphen.
        for label in host.split('.') {
            if label.is_empty() || label.len() > 63 {
                return Err(format!("{raw:?} has a label that is empty or too long"));
            }
            if !label
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                return Err(format!("{raw:?} has a character a hostname cannot hold"));
            }
            if label.starts_with('-') || label.ends_with('-') {
                return Err(format!(
                    "{raw:?} has a label that starts or ends with a hyphen"
                ));
            }
        }

        Ok(Self {
            host: host.to_string(),
            subdomains: wildcard,
            port,
        })
    }

    fn matches(&self, host: &str, port: u16) -> bool {
        if self.port != port {
            return false;
        }
        if !self.subdomains {
            return self.host == host;
        }
        host.strip_suffix(self.host.as_str())
            .is_some_and(|prefix| prefix.ends_with('.') && prefix.len() > 1)
    }
}

/// The hosts a hook may reach over `$http`.
///
/// Empty by default, and empty means **nothing** is reachable: egress from a hook
/// is a capability an operator grants host by host, not one they discover they had.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostAllowlist {
    patterns: Vec<HostPattern>,
}

impl HostAllowlist {
    /// Parse a comma-separated list of hosts (`example.com`, `*.example.com`).
    pub fn parse(raw: &str) -> Result<Self, String> {
        let mut patterns = Vec::new();
        for entry in raw.split(',').map(str::trim).filter(|e| !e.is_empty()) {
            patterns.push(HostPattern::parse(entry)?);
        }
        Ok(Self { patterns })
    }

    /// Whether `host` on `port` is allowed. The comparison is lowercased and ignores
    /// a trailing root dot, so `API.Example.com.` is `api.example.com`; the port must
    /// be the one the entry names (443 unless it names another).
    pub fn allows(&self, host: &str, port: u16) -> bool {
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        if host.is_empty() {
            return false;
        }
        self.patterns
            .iter()
            .any(|pattern| pattern.matches(&host, port))
    }

    /// Whether anything at all is allowed.
    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    /// How many entries, for a log line.
    pub fn len(&self) -> usize {
        self.patterns.len()
    }
}

/// Whether an address is on the public internet.
///
/// This is the check that keeps a hook from reaching the deployment's own network:
/// loopback, link-local (which is where cloud metadata lives, `169.254.169.254`
/// among them), private ranges, carrier-grade NAT, multicast and the reserved
/// blocks are all "not public". An address that is not public is refused even when
/// its host is allowlisted, because an allowlisted hostname can resolve anywhere.
pub fn is_public_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => is_public_v6(v6),
    }
}

fn is_public_v4(address: Ipv4Addr) -> bool {
    // An address that only ever meant something inside another address family is
    // judged by what it spells.
    let [a, b, c, _] = address.octets();
    !(a == 0
        || address.is_private()
        || address.is_loopback()
        || address.is_link_local()
        || address.is_broadcast()
        || address.is_documentation()
        || address.is_multicast()
        || address.is_unspecified()
        // 100.64.0.0/10, carrier-grade NAT.
        || (a == 100 && (64..=127).contains(&b))
        // 192.0.0.0/24, IETF protocol assignments, and 192.88.99.0/24, 6to4 relay.
        || (a == 192 && b == 0 && c == 0)
        || (a == 192 && b == 88 && c == 99)
        // 198.18.0.0/15, benchmarking: it exists to be routed inside a test network.
        || (a == 198 && (b == 18 || b == 19))
        // 240.0.0.0/4 reserved, which includes the limited broadcast address.
        || a >= 240)
}

fn is_public_v6(address: Ipv6Addr) -> bool {
    // An IPv4-mapped address (`::ffff:a.b.c.d`) is that IPv4 address: a host that
    // resolves to one is reaching the IPv4 network, not a private v6 one.
    if let Some(mapped) = address.to_ipv4_mapped() {
        return is_public_v4(mapped);
    }
    // NAT64 (`64:ff9b::/96`) embeds an IPv4 address too; a deployment behind a
    // NAT64 gateway would otherwise reach any v4 address through it.
    let segments = address.segments();
    if segments[0] == 0x0064 && segments[1] == 0xff9b && segments[2..6].iter().all(|s| *s == 0) {
        let [a, b] = [segments[6], segments[7]];
        return is_public_v4(Ipv4Addr::from([
            (a >> 8) as u8,
            (a & 0xff) as u8,
            (b >> 8) as u8,
            (b & 0xff) as u8,
        ]));
    }

    // 6to4 (`2002::/16`) carries an IPv4 address in its next two segments, so a
    // 6to4 address to a private v4 destination has to be judged by that address.
    if segments[0] == 0x2002 {
        let [a, b] = [segments[1], segments[2]];
        return is_public_v4(Ipv4Addr::from([
            (a >> 8) as u8,
            (a & 0xff) as u8,
            (b >> 8) as u8,
            (b & 0xff) as u8,
        ]));
    }

    !(address.is_loopback()
        || address.is_unspecified()
        || address.is_multicast()
        // ::/96, IPv4-compatible (deprecated): the same addresses as `::ffff:0:0/96`
        // without the mapping, and nothing legitimate still uses them.
        || segments[..6].iter().all(|s| *s == 0)
        // fc00::/7, unique local.
        || (segments[0] & 0xfe00) == 0xfc00
        // fec0::/10, site-local (deprecated).
        || (segments[0] & 0xffc0) == 0xfec0
        // fe80::/10, link-local.
        || (segments[0] & 0xffc0) == 0xfe80
        // 2001:db8::/32, documentation.
        || (segments[0] == 0x2001 && segments[1] == 0x0db8)
        // 2001:0000::/32, Teredo: it tunnels through a NAT and hides an obfuscated
        // IPv4 address, which is not something to hand a hook.
        || (segments[0] == 0x2001 && segments[1] == 0x0000)
        // 64:ff9b:1::/48, NAT64 for local use: the embedded v4 is not where the
        // well-known prefix puts it, and the gateway can reach a private one.
        || (segments[0] == 0x0064 && segments[1] == 0xff9b && segments[2] == 0x0001)
        // 100::/64, discard-only.
        || (segments[0] == 0x0100 && segments[1..4].iter().all(|s| *s == 0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(raw: &str) -> IpAddr {
        raw.parse().expect("a test address")
    }

    #[test]
    fn a_single_address_matches_only_itself() {
        let net = TrustedNet::parse("10.0.0.1").expect("parses");
        assert!(net.contains(ip("10.0.0.1")));
        assert!(!net.contains(ip("10.0.0.2")));
    }

    #[test]
    fn a_block_covers_its_range_and_stops_at_the_edge() {
        let net = TrustedNet::parse("10.0.0.0/8").expect("parses");
        assert!(net.contains(ip("10.0.0.1")));
        assert!(net.contains(ip("10.255.255.255")));
        assert!(!net.contains(ip("11.0.0.1")));
        assert!(!net.contains(ip("9.255.255.255")));
    }

    #[test]
    fn a_zero_prefix_covers_the_whole_family() {
        let net = TrustedNet::parse("0.0.0.0/0").expect("parses");
        assert!(net.contains(ip("1.2.3.4")));
        assert!(net.contains(ip("255.255.255.255")));
        // A v4 block does not cover a v6 address.
        assert!(!net.contains(ip("::1")));
    }

    #[test]
    fn v6_blocks_are_masked_too() {
        let net = TrustedNet::parse("fd00::/8").expect("parses");
        assert!(net.contains(ip("fd00::1")));
        assert!(net.contains(ip("fdff::1")));
        assert!(!net.contains(ip("fe00::1")));
        assert!(!net.contains(ip("10.0.0.1")));
    }

    /// A dual-stack proxy reports an IPv4 client as `::ffff:a.b.c.d`.
    #[test]
    fn an_ipv4_mapped_address_is_the_ipv4_address_it_spells() {
        let net = TrustedNet::parse("10.0.0.0/8").expect("parses");
        assert!(net.contains(ip("::ffff:10.0.0.1")));

        // And the reverse: a mapped entry covers the plain address.
        let mapped = TrustedNet::parse("::ffff:10.0.0.0/104").expect("parses");
        assert!(mapped.contains(ip("10.0.0.1")));
    }

    #[test]
    fn a_list_is_parsed_and_blank_entries_are_ignored() {
        let proxies = TrustedProxies::parse(" 10.0.0.1 , , 192.168.0.0/16 ").expect("parses");
        assert_eq!(proxies.len(), 2);
        assert!(proxies.contains(ip("10.0.0.1")));
        assert!(proxies.contains(ip("192.168.5.5")));
        assert!(!proxies.contains(ip("8.8.8.8")));

        let none = TrustedProxies::parse("   ").expect("parses");
        assert!(
            none.is_empty(),
            "nothing configured means nothing is trusted"
        );
    }

    #[test]
    fn a_bad_entry_is_refused_by_name() {
        for raw in ["not-an-ip", "10.0.0.0/33", "", "::/129", "10.0.0.0/abc"] {
            assert!(
                TrustedNet::parse(raw).is_err(),
                "{raw:?} should not parse as a trusted block"
            );
        }
    }

    // ── Host allowlist ──

    #[test]
    fn a_host_entry_allows_that_host_and_nothing_else() {
        let allow = HostAllowlist::parse("api.example.com").expect("parses");
        assert!(allow.allows("api.example.com", 443));
        assert!(allow.allows("API.Example.COM.", 443));
        assert!(!allow.allows("example.com", 443));
        assert!(!allow.allows("evil.example.com", 443));
        assert!(!allow.allows("api.example.com.evil.test", 443));
        assert!(!allow.allows("", 443));
        // The entry grants one port, not every port on that host.
        assert!(!allow.allows("api.example.com", 22));
    }

    #[test]
    fn a_wildcard_allows_subdomains_but_not_the_apex() {
        let allow = HostAllowlist::parse("*.example.com").expect("parses");
        assert!(allow.allows("api.example.com", 443));
        assert!(allow.allows("a.b.example.com", 443));
        // The apex is not a subdomain of itself, and a suffix that is not a label
        // boundary is a different host entirely.
        assert!(!allow.allows("example.com", 443));
        assert!(!allow.allows("notexample.com", 443));
        assert!(!allow.allows("example.com.evil.test", 443));
    }

    #[test]
    fn an_entry_that_is_not_a_host_is_refused() {
        for raw in [
            "https://example.com",
            "example.com/path",
            "user@example.com",
            "10.0.0.1",
            "::1",
            "*.",
            "a..b",
            "-bad.example.com",
        ] {
            assert!(
                HostAllowlist::parse(raw).is_err(),
                "{raw:?} should not parse as an allowed host"
            );
        }
    }

    #[test]
    fn nothing_configured_allows_nothing() {
        let allow = HostAllowlist::parse("  ").expect("parses");
        assert!(allow.is_empty());
        assert!(!allow.allows("example.com", 443));
        assert!(!allow.allows("localhost", 443));
    }

    #[test]
    fn an_entry_with_a_port_grants_only_that_port() {
        let allow = HostAllowlist::parse("hooks.example.com:8443").expect("parses");
        assert!(allow.allows("hooks.example.com", 8443));
        assert!(!allow.allows("hooks.example.com", 443));
        assert!(!allow.allows("hooks.example.com", 22));

        // Two entries for one host, different ports, are separate grants.
        let both =
            HostAllowlist::parse("hooks.example.com, hooks.example.com:8443").expect("parses");
        assert!(both.allows("hooks.example.com", 443));
        assert!(both.allows("hooks.example.com", 8443));
        assert!(!both.allows("hooks.example.com", 8444));
    }

    #[test]
    fn a_bad_port_is_refused() {
        for raw in [
            "example.com:",
            "example.com:0",
            "example.com:http",
            "[::1]:443",
        ] {
            assert!(
                HostAllowlist::parse(raw).is_err(),
                "{raw:?} should not parse as an allowed host"
            );
        }
    }

    #[test]
    fn private_and_link_local_addresses_are_not_public() {
        for raw in [
            "127.0.0.1",
            "127.1.2.3",
            "10.1.2.3",
            "172.16.0.1",
            "172.31.255.254",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "255.255.255.255",
            "224.0.0.1",
            "240.0.0.1",
            "192.0.2.1",
            "198.51.100.7",
            "203.0.113.9",
            "198.18.0.1",
            "198.19.255.254",
            "192.0.0.8",
            "192.88.99.1",
            "::1",
            "::",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "ff02::1",
            "2001:db8::1",
            "100::1",
            "::ffff:127.0.0.1",
            "::ffff:169.254.169.254",
            "64:ff9b::7f00:1",
            "64:ff9b:1::a00:1",
            "2002:7f00:1::",
            "2002:c0a8:0101::",
            "2001:0:4136:e378:8000:63bf:3fff:fdd2",
            "fec0::1",
            "::7f00:1",
        ] {
            assert!(!is_public_ip(ip(raw)), "{raw} must not count as public");
        }
    }

    #[test]
    fn a_public_address_is_public() {
        for raw in [
            "1.1.1.1",
            "8.8.8.8",
            "93.184.216.34",
            "172.15.0.1",
            "172.32.0.1",
            "2606:4700:4700::1111",
            "2001:4860:4860::8888",
            "::ffff:1.1.1.1",
        ] {
            assert!(is_public_ip(ip(raw)), "{raw} should count as public");
        }
    }
}
