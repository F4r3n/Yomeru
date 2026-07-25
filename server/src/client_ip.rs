//! Resolving the real client IP when the server sits behind a reverse proxy.
//!
//! The rate limiters key on client IP. `ConnectInfo<SocketAddr>` gives the
//! *peer* address, which in the documented deployment (nginx → 127.0.0.1:8080,
//! see DEPLOY.md) is always loopback — so every request in production would
//! share a single limiter bucket and the per-IP quotas would degrade into one
//! global quota.
//!
//! Forwarding headers fix that, but only if we refuse to believe them from
//! peers that aren't actually our proxy — otherwise any client could spoof its
//! own address and bypass the limiter entirely. Hence [`TrustProxy`]: headers
//! are consulted only when the *direct peer* is trusted.

use std::net::IpAddr;

use axum::http::HeaderMap;

/// Which peers are allowed to speak for someone else via forwarding headers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrustProxy {
    /// Never read forwarding headers; always use the peer address. Correct when
    /// the server is exposed directly to the internet.
    None,
    /// Trust loopback, RFC1918 / unique-local, and link-local peers. The default:
    /// it covers nginx-on-localhost and container-bridge deployments, while a
    /// public client connecting directly still can't spoof (its peer address is
    /// public, so its headers are ignored).
    Private,
    /// Trust every peer. Only safe when something upstream is guaranteed to
    /// overwrite the forwarding headers on every request.
    All,
}

impl TrustProxy {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" | "off" | "0" | "false" => Some(Self::None),
            "private" | "local" => Some(Self::Private),
            "all" | "any" => Some(Self::All),
            _ => None,
        }
    }

    fn trusts(self, peer: IpAddr) -> bool {
        match self {
            Self::None => false,
            Self::All => true,
            Self::Private => is_private(peer),
        }
    }
}

/// Loopback / private / link-local, after unwrapping IPv4-mapped IPv6 (a
/// dual-stack listener reports `::ffff:127.0.0.1` for a v4 loopback peer).
fn is_private(ip: IpAddr) -> bool {
    match ip.to_canonical() {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => {
            let seg0 = v6.segments()[0];
            // fc00::/7 unique-local, fe80::/10 link-local.
            v6.is_loopback() || (seg0 & 0xfe00) == 0xfc00 || (seg0 & 0xffc0) == 0xfe80
        }
    }
}

/// The client address to rate-limit on.
///
/// `hops` is how many trusted proxies sit in front of us. `X-Forwarded-For` is
/// append-only (nginx's `$proxy_add_x_forwarded_for` tacks the peer onto
/// whatever the client sent), so everything left of our own proxies' entries is
/// attacker-controlled. Counting `hops` entries in from the *right* lands on the
/// address our outermost trusted proxy observed.
pub fn client_ip(peer: IpAddr, headers: &HeaderMap, trust: TrustProxy, hops: usize) -> IpAddr {
    let peer = peer.to_canonical();
    if !trust.trusts(peer) {
        return peer;
    }
    forwarded_for(headers, hops)
        .or_else(|| real_ip(headers))
        .unwrap_or(peer)
}

fn forwarded_for(headers: &HeaderMap, hops: usize) -> Option<IpAddr> {
    let raw = headers.get("x-forwarded-for")?.to_str().ok()?;
    let entries: Vec<&str> = raw
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    // hops=1 → last entry (the one our proxy appended).
    let idx = entries.len().checked_sub(hops.max(1))?;
    parse_ip(entries.get(idx)?)
}

/// nginx sets `X-Real-IP` with `proxy_set_header`, which *overwrites* any
/// client-supplied value, so a single-hop deploy can rely on it directly.
fn real_ip(headers: &HeaderMap) -> Option<IpAddr> {
    parse_ip(headers.get("x-real-ip")?.to_str().ok()?.trim())
}

fn parse_ip(s: &str) -> Option<IpAddr> {
    // Tolerate the `[v6]:port` / `v4:port` forms some proxies emit.
    let s = s.trim();
    if let Ok(ip) = s.parse::<IpAddr>() {
        return Some(ip.to_canonical());
    }
    if let Some(rest) = s.strip_prefix('[') {
        let (host, _) = rest.split_once(']')?;
        return host.parse::<IpAddr>().ok().map(|ip| ip.to_canonical());
    }
    let (host, _) = s.rsplit_once(':')?;
    host.parse::<IpAddr>().ok().map(|ip| ip.to_canonical())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROXY: IpAddr = IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);
    const PUBLIC: IpAddr = IpAddr::V4(std::net::Ipv4Addr::new(203, 0, 113, 7));

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        use axum::http::header::{HeaderName, HeaderValue};
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        h
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn uses_peer_when_no_headers() {
        let h = headers(&[]);
        assert_eq!(client_ip(PROXY, &h, TrustProxy::Private, 1), PROXY);
    }

    #[test]
    fn reads_forwarded_for_from_trusted_peer() {
        let h = headers(&[("x-forwarded-for", "198.51.100.9")]);
        assert_eq!(
            client_ip(PROXY, &h, TrustProxy::Private, 1),
            ip("198.51.100.9")
        );
    }

    #[test]
    fn ignores_headers_from_untrusted_peer() {
        // The whole point: a client connecting directly cannot claim to be
        // someone else and escape its own rate-limit bucket.
        let h = headers(&[("x-forwarded-for", "1.2.3.4"), ("x-real-ip", "1.2.3.4")]);
        assert_eq!(client_ip(PUBLIC, &h, TrustProxy::Private, 1), PUBLIC);
    }

    #[test]
    fn spoofed_prefix_is_ignored_at_one_hop() {
        // nginx appends the true peer, so the rightmost entry is the real one
        // and the attacker-supplied prefix must not win.
        let h = headers(&[("x-forwarded-for", "9.9.9.9, 198.51.100.9")]);
        assert_eq!(
            client_ip(PROXY, &h, TrustProxy::Private, 1),
            ip("198.51.100.9")
        );
    }

    #[test]
    fn counts_hops_from_the_right() {
        // Two trusted hops (CDN → nginx): the CDN's own address is rightmost,
        // the client is one further left.
        let h = headers(&[("x-forwarded-for", "9.9.9.9, 198.51.100.9, 10.0.0.5")]);
        assert_eq!(
            client_ip(PROXY, &h, TrustProxy::Private, 2),
            ip("198.51.100.9")
        );
    }

    #[test]
    fn falls_back_to_real_ip() {
        let h = headers(&[("x-real-ip", "198.51.100.9")]);
        assert_eq!(
            client_ip(PROXY, &h, TrustProxy::Private, 1),
            ip("198.51.100.9")
        );
    }

    #[test]
    fn trust_none_always_uses_peer() {
        let h = headers(&[("x-forwarded-for", "198.51.100.9")]);
        assert_eq!(client_ip(PROXY, &h, TrustProxy::None, 1), PROXY);
    }

    #[test]
    fn container_bridge_peer_is_trusted() {
        // Docker publishes ports via a bridge, so the peer is 172.17.0.1 rather
        // than loopback — this must still count as "our proxy".
        let h = headers(&[("x-forwarded-for", "198.51.100.9")]);
        assert_eq!(
            client_ip(ip("172.17.0.1"), &h, TrustProxy::Private, 1),
            ip("198.51.100.9")
        );
    }

    #[test]
    fn ipv4_mapped_loopback_peer_is_trusted() {
        let h = headers(&[("x-forwarded-for", "198.51.100.9")]);
        assert_eq!(
            client_ip(ip("::ffff:127.0.0.1"), &h, TrustProxy::Private, 1),
            ip("198.51.100.9")
        );
    }

    #[test]
    fn strips_port_suffixes() {
        let h = headers(&[("x-forwarded-for", "198.51.100.9:51234")]);
        assert_eq!(
            client_ip(PROXY, &h, TrustProxy::Private, 1),
            ip("198.51.100.9")
        );
        let h = headers(&[("x-forwarded-for", "[2001:db8::1]:443")]);
        assert_eq!(
            client_ip(PROXY, &h, TrustProxy::Private, 1),
            ip("2001:db8::1")
        );
    }

    #[test]
    fn malformed_header_falls_back_to_peer() {
        let h = headers(&[("x-forwarded-for", "not-an-ip")]);
        assert_eq!(client_ip(PROXY, &h, TrustProxy::Private, 1), PROXY);
    }

    #[test]
    fn parses_trust_settings() {
        assert_eq!(TrustProxy::parse("none"), Some(TrustProxy::None));
        assert_eq!(TrustProxy::parse("Private"), Some(TrustProxy::Private));
        assert_eq!(TrustProxy::parse("all"), Some(TrustProxy::All));
        assert_eq!(TrustProxy::parse("nonsense"), None);
    }
}
