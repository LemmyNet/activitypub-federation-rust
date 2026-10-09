//! DNS resolver with IP checks, and url validation helpers

use crate::error::Error;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use std::{
    env,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
};
use tokio::net::lookup_host;
use url::{Host, Url};

/// Resolve the given hostname (or IP literal) and return the resolved addresses, so they can be
/// used directly for connecting. If `validate` is true, throw an error if any address points to
/// an internal IP. `validate` should be false in debug mode, so that local urls can be fetched
/// for testing.
async fn resolve_and_validate_host(
    host: &str,
    port: Option<u16>,
    allow_hosts: &[(String, u16)],
    validate: bool,
) -> Result<Vec<SocketAddr>, Error> {
    let host = normalize_host(host);
    // Port is only used to construct the SocketAddr; it is overwritten when connecting.
    let default_port = port.unwrap_or(0);
    // Parse host as plain IP, otherwise resolve domain via DNS
    let addrs: Vec<SocketAddr> = match host.parse::<IpAddr>() {
        Ok(ip) => vec![SocketAddr::new(ip.to_canonical(), default_port)],
        Err(_) => lookup_host((host.as_str(), default_port)).await?.collect(),
    };

    let is_allowed_host = allow_hosts.iter().any(|(allowed_host, allowed_port)| {
        allowed_host == &host && (port.is_none() || Some(allowed_port) == port.as_ref())
    });
    if !validate || env::var("DANGER_FEDERATION_ALLOW_LOCAL_IP").is_ok() || is_allowed_host {
        return Ok(addrs);
    }

    if let Some(ip) = addrs
        .iter()
        .map(|a| a.ip().to_canonical())
        .find(|ip| match ip {
            IpAddr::V4(ipv4) => v4_is_invalid(*ipv4),
            IpAddr::V6(ipv6) => v6_is_invalid(*ipv6),
        })
    {
        return Err(Error::ResolveInternalIpError(host, ip));
    }
    Ok(addrs)
}

/// Throw an error if the url points to an internal IP, either as literal IP or as domain which
/// resolves to an internal IP. Also checks that allowlisted hosts are only accessed on their
/// configured port, since the DNS resolver has no access to the port. Validation is skipped if
/// `validate` is false (e.g. debug mode), see [`resolve_and_validate_host()`].
pub async fn validate_url(url: &Url, allow_hosts: &[Url], validate: bool) -> Result<(), Error> {
    let host = match url
        .host()
        .ok_or(Error::UrlVerificationError("Url must have a domain"))?
    {
        Host::Domain(domain) => domain.to_owned(),
        Host::Ipv4(ipv4) => ipv4.to_string(),
        Host::Ipv6(ipv6) => ipv6.to_string(),
    };
    resolve_and_validate_host(
        &host,
        url.port_or_known_default(),
        &allow_host_pairs(allow_hosts),
        validate,
    )
    .await?;
    Ok(())
}

/// Normalize allowlist urls into (host, port) pairs.
fn allow_host_pairs(allow_hosts: &[Url]) -> Vec<(String, u16)> {
    allow_hosts
        .iter()
        .filter_map(|url| {
            let host = normalize_host(url.host_str()?);
            Some((host, url.port_or_known_default().unwrap_or(80)))
        })
        .collect()
}

/// DNS resolver which rejects hostnames that resolve to internal IPs.
///
/// Attach it to your HTTP client via `reqwest::ClientBuilder::dns_resolver`, so that IP
/// validation happens at the exact moment of connecting. This closes the gap which
/// otherwise lets attackers bypass IP checks with DNS rebinding (short TTL records
/// switching between public and internal IPs).
///
/// Note that URLs with a literal IP (e.g. `http://127.0.0.1/`) never trigger DNS resolution
/// and need to be checked separately (see [`validate_url()`]).
pub struct ValidatingDnsResolver {
    /// Hosts (with port) which are exempt from validation
    allow_hosts: Vec<(String, u16)>,
    /// If false validation is skipped
    validate: bool,
}

impl ValidatingDnsResolver {
    /// Create a resolver which validates all hostnames except the given `allow_hosts`.
    /// Ports and trailing dots in the given hosts are ignored.
    pub fn new(allow_hosts: Vec<Url>, validate: bool) -> Self {
        Self {
            allow_hosts: allow_host_pairs(&allow_hosts),
            validate,
        }
    }

    fn allows(&self, host: &str) -> bool {
        let host = normalize_host(host);
        self.allow_hosts.iter().any(|(h, _)| h == &host)
    }
}

impl Resolve for ValidatingDnsResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().trim_end_matches('.').to_owned();
        let validate = self.validate && !self.allows(&host);
        let allow_hosts = self.allow_hosts.clone();
        Box::pin(async move {
            // DNS resolver has no access to the port, so allowlisted hosts are matched by
            // name only. Port check is handled in `validate_url`.
            let addrs: Vec<SocketAddr> =
                resolve_and_validate_host(&host, None, &allow_hosts, validate).await?;
            let addrs: Addrs = Box::new(addrs.into_iter());
            Ok(addrs)
        })
    }
}

/// Strips trailing dot and port from a hostname, lowercases it. Preserves bare IPv6 addresses.
fn normalize_host(host: &str) -> String {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if let Some(stripped) = host.strip_prefix('[') {
        if let Some((ipv6, _)) = stripped.split_once(']') {
            return ipv6.to_owned();
        }
    }
    // An IPv6 address contains multiple colons, dont strip anything in that case.
    if host.matches(':').count() > 1 {
        return host;
    }
    host.split(':').next().unwrap_or(&host).to_owned()
}

/// Checks if the address is in a non-global range, mirroring
/// `std::net::Ipv4Addr::is_global()` (unstable, see https://github.com/rust-lang/rust/issues/27709).
/// Ranges which cannot connect to internal services (e.g. IETF protocol assignments,
/// benchmarking) are not checked.
fn v4_is_invalid(v4: Ipv4Addr) -> bool {
    let octets = v4.octets();
    v4.is_private()
        || v4.is_loopback()
        || v4.is_link_local()
        || v4.is_multicast()
        || v4.is_documentation()
        || v4.is_unspecified()
        || v4.is_broadcast()
        // 0.0.0.0/8: connecting to 0.x.x.x reaches loopback on Linux
        || octets[0] == 0
        // 100.64.0.0/10: shared address space (CGNAT), used for internal services by cloud providers
        || (octets[0] == 100 && (octets[1] & 0xc0) == 0x40)
        || octets[0] >= 240 // reserved 240.0.0.0/4
}

/// Checks if the address is in a non-global range, mirroring
/// `std::net::Ipv6Addr::is_global()` (unstable, see https://github.com/rust-lang/rust/issues/27709).
/// Ranges which cannot connect to internal services (e.g. Teredo, discard-only) are not checked.
fn v6_is_invalid(v6: Ipv6Addr) -> bool {
    let segments = v6.segments();
    // NAT64 well-known prefix (64:ff9b::/96) embeds an IPv4 address in the low 32 bits.
    // Validate the embedded address instead of blocking the prefix, so that IPv6-only
    // hosts using DNS64/NAT64 keep working.
    if matches!(segments, [0x64, 0xff9b, 0, 0, 0, 0, ..]) {
        let [a, b] = segments[6].to_be_bytes();
        let [c, d] = segments[7].to_be_bytes();
        return v4_is_invalid(Ipv4Addr::from([a, b, c, d]));
    }
    matches!(
        segments,
        // documentation (2001:db8::/32, 3fff::/20)
        [0x2001, 0xdb8, ..] | [0x3fff, 0..=0x0fff, ..]
        // IPv4-compatible (::/96, deprecated) and IPv4-mapped (::ffff:0:0/96)
        | [0, 0, 0, 0, 0, 0, ..]
        | [0, 0, 0, 0, 0, 0xffff, ..]
        // local-use NAT64 prefix (64:ff9b:1::/48)
        | [0x64, 0xff9b, 1, ..]
        // 6to4 (2002::/16, deprecated)
        | [0x2002, ..]
    ) || v6.is_loopback()
        || v6.is_multicast()
        || v6.is_unique_local()
        || v6.is_unicast_link_local()
        || v6.is_unspecified()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod test {
    use super::*;

    #[tokio::test]
    async fn test_validate_url() -> Result<(), Error> {
        assert!(validate_url(&Url::parse("http://example.com")?, &[], true)
            .await
            .is_ok());
        assert!(
            validate_url(&Url::parse("http://172.66.147.243")?, &[], true)
                .await
                .is_ok()
        );
        assert!(validate_url(&Url::parse("http://localhost")?, &[], true)
            .await
            .is_err());
        assert!(validate_url(&Url::parse("http://127.0.0.1")?, &[], true)
            .await
            .is_err());

        // allowlisted host is only allowed on the configured port
        let allow_hosts = vec![Url::parse("http://localhost:8080")?];
        let url = Url::parse("http://localhost:8080/")?;
        assert!(validate_url(&url, &allow_hosts, true).await.is_ok());
        let url = Url::parse("http://localhost:9000/")?;
        assert!(validate_url(&url, &allow_hosts, true).await.is_err());
        let url = Url::parse("http://localhost/")?;
        assert!(validate_url(&url, &allow_hosts, true).await.is_err());
        let url = Url::parse("http://127.0.0.1:8080/")?;
        assert!(validate_url(&url, &allow_hosts, true).await.is_err());

        // debug mode skips validation
        let url = Url::parse("http://localhost/")?;
        assert!(validate_url(&url, &[], false).await.is_ok());
        let url = Url::parse("http://localhost:9000/")?;
        assert!(validate_url(&url, &allow_hosts, false).await.is_ok());
        Ok(())
    }

    #[test]
    fn test_invalid_ipv4() {
        for ip in [
            "0.0.0.1",    // loopback on Linux
            "10.0.0.1",   // private
            "100.64.0.1", // CGNAT
            "100.127.255.254",
            "127.0.0.1",       // loopback
            "169.254.1.1",     // link-local
            "172.16.0.1",      // private
            "192.0.2.1",       // documentation
            "224.0.0.1",       // multicast
            "240.0.0.1",       // reserved
            "255.255.255.255", // broadcast
        ] {
            assert!(v4_is_invalid(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn test_valid_ipv4() {
        for ip in [
            "1.2.3.4",
            "8.8.8.8",
            "100.128.0.1", // above CGNAT
            "192.0.0.9",
            "192.0.0.10",
            "192.0.0.1",   // IETF protocol assignments
            "192.88.99.1", // 6to4 relay anycast
            "198.18.0.1",  // benchmarking
        ] {
            assert!(!v4_is_invalid(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn test_invalid_ipv6() {
        for ip in [
            "::1",              // loopback
            "fc00::1",          // unique local
            "fe80::1",          // link-local
            "ff01::1",          // multicast
            "2001:db8::1",      // documentation
            "3fff:10::1",       // documentation
            "::ffff:127.0.0.1", // IPv4-mapped
            "::7f00:1",         // IPv4-compatible
            "64:ff9b::7f00:1",  // NAT64, embedded loopback
            "64:ff9b::10.0.0.1",
            "64:ff9b:1::1",    // local-use NAT64
            "2002:7f00:1::",   // 6to4
            "2002:c0a8:101::", // 6to4
        ] {
            assert!(v6_is_invalid(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn test_valid_ipv6() {
        for ip in [
            "2606:4700::1",
            // NAT64 with public IP, needed for IPv6-only hosts using DNS64
            "64:ff9b::801:1010",
            // IETF protocol assignment anycast
            "2001:1::1",
            "2001:3::1",
            "2001:20::1",
            "2001::1",     // Teredo
            "2001:100::1", // IETF protocol assignments
            "100::1",      // discard-only
            "5f00::1",     // segment routing
        ] {
            assert!(!v6_is_invalid(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn test_normalize_host() {
        assert_eq!("example.com", normalize_host("example.com"));
        assert_eq!("example.com", normalize_host("EXAMPLE.COM."));
        assert_eq!("localhost", normalize_host("localhost:8080"));
        assert_eq!("::1", normalize_host("[::1]:8080"));
        assert_eq!("::1", normalize_host("[::1]"));
        assert_eq!("::1", normalize_host("::1"));
        assert_eq!("2001:db8::1", normalize_host("2001:DB8::1."));
    }

    #[test]
    fn test_resolver_allows() {
        let resolver = ValidatingDnsResolver::new(
            vec![
                Url::parse("http://localhost:8080").unwrap(),
                Url::parse("https://lemmy.example.com").unwrap(),
            ],
            true,
        );
        assert!(resolver.allows("localhost"));
        assert!(resolver.allows("lemmy.example.com"));
        assert!(resolver.allows("lemmy.example.com."));
        assert!(!resolver.allows("internal.attacker.com"));
        // `allows` only checks the host list; skipping validation is handled via the
        // `validate` flag passed to `resolve_and_validate_host`
        let resolver = ValidatingDnsResolver::new(vec![], false);
        assert!(!resolver.allows("internal.attacker.com"));
    }
}
