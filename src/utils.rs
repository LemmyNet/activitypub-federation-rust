use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::error::Error;
use tokio::net::lookup_host;
use url::{Host, Url};

// TODO: Use is_global() once stabilized
//       https://doc.rust-lang.org/std/net/enum.IpAddr.html#method.is_global
pub(crate) async fn validate_ip(url: &Url) -> Result<(), Error> {
    let mut ip = vec![];
    let host = url
        .host()
        .ok_or(Error::UrlVerificationError("Url must have a domain"))?;
    match host {
        Host::Domain(domain) => ip.extend(
            lookup_host((domain.to_owned(), 80))
                .await?
                .map(|s| s.ip().to_canonical()),
        ),
        Host::Ipv4(ipv4) => ip.push(ipv4.into()),
        Host::Ipv6(ipv6) => ip.push(ipv6.into()),
    };

    let invalid_ip = ip.into_iter().any(|addr| match addr {
        IpAddr::V4(addr) => v4_is_invalid(addr),
        IpAddr::V6(addr) => v6_is_invalid(addr),
    });
    if invalid_ip {
        return Err(Error::DomainResolveError(host.to_string()));
    }
    Ok(())
}

fn v4_is_invalid(v4: Ipv4Addr) -> bool {
    v4.is_private()
        || v4.is_loopback()
        || v4.is_link_local()
        || v4.is_multicast()
        || v4.is_documentation()
        || v4.is_unspecified()
        || v4.is_broadcast()
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
        assert!(validate_ip(&Url::parse("http://example.com")?)
            .await
            .is_ok());
        assert!(validate_ip(&Url::parse("http://172.66.147.243")?)
            .await
            .is_ok());
        assert!(validate_ip(&Url::parse("http://localhost")?).await.is_err());
        assert!(validate_ip(&Url::parse("http://127.0.0.1")?).await.is_err());
        Ok(())
    }
}
