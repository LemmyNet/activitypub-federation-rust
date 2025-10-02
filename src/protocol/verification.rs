//! Verify that received data is valid

use crate::{config::Data, error::Error, fetch::object_id::ObjectId, traits::Object};
use serde::Deserialize;
use url::Url;

/// Check that both urls have the same domain. If not, return UrlVerificationError.
///
/// ```
/// # use url::Url;
/// # use activitypub_federation::protocol::verification::verify_domains_match;
/// let a = Url::parse("https://example.com/abc")?;
/// let b = Url::parse("https://sample.net/abc")?;
/// assert!(verify_domains_match(&a, &b).is_err());
/// # Ok::<(), url::ParseError>(())
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
/// # use url::Url;
/// # use activitypub_federation::protocol::verification::verify_urls_match;
/// let a = Url::parse("https://example.com/abc")?;
/// let b = Url::parse("https://example.com/123")?;
/// assert!(verify_urls_match(&a, &b).is_err());
/// # Ok::<(), url::ParseError>(())
/// ```
pub fn verify_urls_match(a: &Url, b: &Url) -> Result<(), Error> {
    if a != b {
        return Err(Error::UrlVerificationError("Urls do not match"));
    }
    Ok(())
}

/// Check that the given ID doesn't match the local domain.
///
/// It is important to verify this to avoid local objects from being overwritten. In general
/// locally created objects should be considered authorative, while incoming federated data
/// is untrusted. Lack of such a check could allow an attacker to rewrite local posts. It could
/// also result in an `object.local` field being overwritten with `false` for local objects, resulting in invalid data.
///
/// ```
/// # use activitypub_federation::fetch::object_id::ObjectId;
/// # use activitypub_federation::config::FederationConfig;
/// # use activitypub_federation::protocol::verification::verify_is_remote_object;
/// # use activitypub_federation::traits::tests::{DbConnection, DbUser};
/// # tokio::runtime::Runtime::new().unwrap().block_on(async {
/// # let config = FederationConfig::builder().domain("example.com").app_data(DbConnection).build().await?;
/// # let data = config.to_request_data();
/// let id = ObjectId::<DbUser>::parse("https://remote.com/u/name")?;
/// assert!(verify_is_remote_object(&id, &data).is_ok());
/// # Ok::<(), anyhow::Error>(())
/// # }).unwrap();
/// ```
pub fn verify_is_remote_object<Kind, R: Clone>(
    id: &ObjectId<Kind>,
    data: &Data<<Kind as Object>::DataType>,
) -> Result<(), Error>
where
    Kind: Object<DataType = R> + Send + Sync + 'static,
    for<'de2> <Kind as Object>::Kind: Deserialize<'de2>,
{
    if id.is_local(data) {
        Err(Error::UrlVerificationError("Object is not remote"))
    } else {
        Ok(())
    }
}
