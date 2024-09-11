//! Verify that received data is valid

use crate::{error::Error, url::Url};

/// Check that both urls have the same domain. If not, return UrlVerificationError.
///
/// ```
/// # use crate::url::Url;
/// # use activitypub_federation::protocol::verification::verify_domains_match;
/// let a = Url::from_str("https://example.com/abc")?;
/// let b = Url::from_str("https://sample.net/abc")?;
/// assert!(verify_domains_match(&a, &b).is_err());
/// # Ok::<(), Url::from_strError>(())
/// ```
pub fn verify_domains_match(a: &Url, b: &Url) -> Result<(), Error> {
    if a.domain() != b.domain() {
        return Err(Error::UrlVerificationError("Domains do not match"));
    }
    Ok(())
}

/// Check that both urls are identical. If not, return UrlVerificationError.
///
/// ```
/// # use crate::url::Url;
/// # use activitypub_federation::protocol::verification::verify_urls_match;
/// let a = Url::from_str("https://example.com/abc")?;
/// let b = Url::from_str("https://example.com/123")?;
/// assert!(verify_urls_match(&a, &b).is_err());
/// # Ok::<(), Url::from_strError>(())
/// ```
pub fn verify_urls_match(a: &Url, b: &Url) -> Result<(), Error> {
    if a != b {
        return Err(Error::UrlVerificationError("Urls do not match"));
    }
    Ok(())
}
