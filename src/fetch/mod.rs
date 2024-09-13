//! Utilities for fetching data from other servers
//!
#![doc = include_str!("../../docs/07_fetching_data.md")]

use crate::{
    config::Data,
    error::{Error, Error::ParseFetchedObject},
    extract_id,
    http_signatures::sign_request,
    reqwest_shim::ResponseExt,
    FEDERATION_CONTENT_TYPE,
};
use bytes::Bytes;
use http::{HeaderValue, StatusCode};
use serde::de::DeserializeOwned;
use std::sync::atomic::Ordering;
use tracing::info;
use url::Url;

/// Typed wrapper for collection IDs
pub mod collection_id;
/// Typed wrapper for Activitypub Object ID which helps with dereferencing and caching
pub mod object_id;
/// Resolves identifiers of the form `name@example.com`
pub mod webfinger;

/// Response from fetching a remote object
pub struct FetchObjectResponse<Kind> {
    /// The resolved object
    pub object: Kind,
    /// Contains the final URL (different from request URL in case of redirect)
    pub url: Url,
    content_type: Option<HeaderValue>,
    object_id: Option<Url>,
}

/// Fetch a remote object over HTTP and convert to `Kind`.
///
/// [crate::fetch::object_id::ObjectId::dereference] wraps this function to add caching and
/// conversion to database type. Only use this function directly in exceptional cases where that
/// behaviour is undesired.
///
/// Every time an object is fetched via HTTP, [RequestData.request_counter] is incremented by one.
/// If the value exceeds [FederationSettings.http_fetch_limit], the request is aborted with
/// [Error::RequestLimit]. This prevents denial of service attacks where an attack triggers
/// infinite, recursive fetching of data.
///
/// The `Accept` header will be set to the content of [`FEDERATION_CONTENT_TYPE`]. When parsing the
/// response it ensures that it has a valid `Content-Type` header as defined by ActivityPub, to
/// prevent security vulnerabilities like [this one](https://github.com/mastodon/mastodon/security/advisories/GHSA-jhrq-qvrm-qr36).
/// Additionally it checks that the `id` field is identical to the fetch URL (after redirects).
pub async fn fetch_object_http<T: Clone, Kind: DeserializeOwned>(
    url: &Url,
    data: &Data<T>,
) -> Result<FetchObjectResponse<Kind>, Error> {
    static FETCH_CONTENT_TYPE: HeaderValue = HeaderValue::from_static(FEDERATION_CONTENT_TYPE);
    const VALID_RESPONSE_CONTENT_TYPES: [&str; 3] = [
        FEDERATION_CONTENT_TYPE, // lemmy
        r#"application/ld+json; profile="https://www.w3.org/ns/activitystreams""#, // activitypub standard
        r#"application/activity+json; charset=utf-8"#,                             // mastodon
    ];
    let res = fetch_object_http_with_accept(url, data, &FETCH_CONTENT_TYPE).await?;

    // Ensure correct content-type to prevent vulnerabilities, with case insensitive comparison.
    let content_type = res
        .content_type
        .as_ref()
        .and_then(|c| Some(c.to_str().ok()?.to_lowercase()))
        .ok_or(Error::FetchInvalidContentType(res.url.clone()))?;
    if !VALID_RESPONSE_CONTENT_TYPES.contains(&content_type.as_str()) {
        return Err(Error::FetchInvalidContentType(res.url));
    }

    // Ensure id field matches final url after redirect
    if res.object_id.as_ref() != Some(&res.url) {
        if let Some(res_object_id) = res.object_id {
            // If id is different but still on the same domain, attempt to request object
            // again from url in id field.
            if res_object_id.domain() == res.url.domain() {
                return Box::pin(fetch_object_http(&res_object_id, data)).await;
            }
        }
        // Failed to fetch the object from its specified id
        return Err(Error::FetchWrongId(res.url));
    }

    // Dont allow fetching local object. Only check this after the request as a local url
    // may redirect to a remote object.
    if data.config.is_local_url(&res.url) {
        return Err(Error::NotFound);
    }

    Ok(res)
}

/// Fetch a remote object over HTTP and convert to `Kind`. This function works exactly as
/// [`fetch_object_http`] except that the `Accept` header is specified in `content_type`.
async fn fetch_object_http_with_accept<T: Clone, Kind: DeserializeOwned>(
    url: &Url,
    data: &Data<T>,
    content_type: &HeaderValue,
) -> Result<FetchObjectResponse<Kind>, Error> {
    let config = &data.config;
    config.verify_url_valid(url).await?;
    info!("Fetching remote object {}", url.to_string());

    let mut counter = data.request_counter.fetch_add(1, Ordering::SeqCst);
    // fetch_add returns old value so we need to increment manually here
    counter += 1;
    if counter > config.http_fetch_limit {
        return Err(Error::RequestLimit);
    }

    let req = config
        .client
        .get(url.as_str())
        .header("Accept", content_type)
        .timeout(config.request_timeout);

    let res = if let Some((actor_id, private_key_pem)) = config.signed_fetch_actor.as_deref() {
        let req = sign_request(
            req,
            actor_id,
            Bytes::new(),
            private_key_pem.clone(),
            data.config.http_signature_compat,
        )
        .await?;
        config.client.execute(req).await?
    } else {
        req.send().await?
    };

    if res.status() == StatusCode::GONE {
        return Err(Error::ObjectDeleted(url.clone()));
    }

    let url = res.url().clone();
    let content_type = res.headers().get("Content-Type").cloned();
    let text = res.bytes_limited().await?;
    let object_id = extract_id(&text).ok();

    match serde_json::from_slice(&text) {
        Ok(object) => Ok(FetchObjectResponse {
            object,
            url,
            content_type,
            object_id,
        }),
        Err(e) => Err(ParseFetchedObject(
            e,
            url,
            String::from_utf8(Vec::from(text))?,
        )),
    }
}
