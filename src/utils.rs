use crate::{
    request_data::RequestData,
    utils::reqwest_shim::ResponseExt,
    Error,
    APUB_JSON_CONTENT_TYPE,
};
use http::{header::HeaderName, HeaderValue, StatusCode};
use serde::de::DeserializeOwned;
use std::{collections::BTreeMap, sync::atomic::Ordering};
use tracing::info;
use url::Url;

pub(crate) mod reqwest_shim;

pub async fn fetch_object_http<T, Kind: DeserializeOwned>(
    url: &Url,
    data: &RequestData<T>,
) -> Result<Kind, Error> {
    let instance = &data.local_instance();
    // dont fetch local objects this way
    debug_assert!(url.domain() != Some(&instance.hostname));
    instance.verify_url_valid(url).await?;
    info!("Fetching remote object {}", url.to_string());

    let counter = data.request_counter.fetch_add(1, Ordering::SeqCst);
    if counter > instance.settings.http_fetch_limit {
        return Err(Error::RequestLimit);
    }

    let res = instance
        .client
        .get(url.as_str())
        .header("Accept", APUB_JSON_CONTENT_TYPE)
        .timeout(instance.settings.request_timeout)
        .send()
        .await
        .map_err(Error::conv)?;

    if res.status() == StatusCode::GONE {
        return Err(Error::ObjectDeleted);
    }

    res.json_limited().await
}

/// Check that both urls have the same domain. If not, return UrlVerificationError.
pub fn verify_domains_match(a: &Url, b: &Url) -> Result<(), Error> {
    if a.domain() != b.domain() {
        return Err(Error::UrlVerificationError("Domains do not match"));
    }
    Ok(())
}

/// Check that both urls are identical. If not, return UrlVerificationError.
pub fn verify_urls_match(a: &Url, b: &Url) -> Result<(), Error> {
    if a != b {
        return Err(Error::UrlVerificationError("Urls do not match"));
    }
    Ok(())
}

/// Utility to converts either actix or axum headermap to a BTreeMap
pub fn header_to_map<'a, H>(headers: H) -> BTreeMap<String, String>
where
    H: IntoIterator<Item = (&'a HeaderName, &'a HeaderValue)>,
{
    let mut header_map = BTreeMap::new();

    for (name, value) in headers {
        if let Ok(value) = value.to_str() {
            header_map.insert(name.to_string(), value.to_string());
        }
    }

    header_map
}
