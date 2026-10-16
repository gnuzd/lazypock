//! Which peers are allowed to speak for a client.
//!
//! A forwarding header is set by the client when there is no proxy in front, so
//! believing one unconditionally means believing whatever the caller wrote. The
//! only safe rule is: read it, but only when the connection itself came from a
//! proxy you named.
//!
//! This is the whole of that rule's data — a list of addresses and CIDR blocks,
//! parsed from `LAZYPOCK_TRUSTED_PROXIES`.

use std::net::IpAddr;

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
}
