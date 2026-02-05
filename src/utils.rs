use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::error::Error;
use tokio::net::lookup_host;

// Resolve domain and see if it points to private IP
// TODO: Use is_global() once stabilized
//       https://doc.rust-lang.org/std/net/enum.IpAddr.html#method.is_global
pub(crate) async fn is_invalid_ip(domain: &str) -> Result<bool, Error> {
    let mut ips = lookup_host((domain, 80)).await?;
    Ok(ips.any(|addr| match addr.ip() {
        IpAddr::V4(addr) => v4_is_invalid(addr),
        IpAddr::V6(addr) => v6_is_invalid(addr),
    }))
}

fn v4_is_invalid(v4: Ipv4Addr) -> bool {
    v4.is_private()
        || v4.is_loopback()
        || v4.is_link_local()
        || v4.is_multicast()
        || v4.is_documentation()
}

fn v6_is_invalid(v6: Ipv6Addr) -> bool {
    v6.is_loopback()
        || v6.is_multicast()
        || v6.is_unique_local()
        || v6.is_unicast_link_local()
        || v6.is_unspecified()
        || v6_is_documentation(v6)
        || v6.to_ipv4_mapped().is_some_and(v4_is_invalid)
}

fn v6_is_documentation(v6: std::net::Ipv6Addr) -> bool {
    matches!(
        v6.segments(),
        [0x2001, 0xdb8, ..] | [0x3fff, 0..=0x0fff, ..]
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod test {
    use super::*;

    #[tokio::test]
    async fn test_is_valid_ip() -> Result<(), Error> {
        assert!(!is_invalid_ip("example.com").await?);
        assert!(is_invalid_ip("localhost").await?);
        Ok(())
    }
}
