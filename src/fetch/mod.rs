//! Utilities for fetching data from other servers
//!
#![doc = include_str!("../../docs/07_fetching_data.md")]

use crate::{
    config::Data,
    error::Error,
    http_signatures::sign_request,
    reqwest_shim::ResponseExt,
    traits::Actor,
    FEDERATION_CONTENT_TYPE,
};
use anyhow::anyhow;
use http::StatusCode;
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
pub async fn fetch_object_http<T: Clone, Kind: DeserializeOwned>(
    url: &Url,
    data: &Data<T>,
) -> Result<Kind, Error> {
    let config = &data.config;
    // dont fetch local objects this way
    debug_assert!(url.domain() != Some(&config.domain));
    config.verify_url_valid(url).await?;
    info!("Fetching remote object {}", url.to_string());

    let counter = data.request_counter.fetch_add(1, Ordering::SeqCst);
    if counter > config.http_fetch_limit {
        return Err(Error::RequestLimit);
    }

    let res = config
        .client
        .get(url.as_str())
        .header("Accept", FEDERATION_CONTENT_TYPE)
        .timeout(config.request_timeout)
        .send()
        .await
        .map_err(Error::other)?;

    if res.status() == StatusCode::GONE {
        return Err(Error::ObjectDeleted);
    }

    res.json_limited().await
}

/// Signed version of `fetch_object_http_signed`. This will sign the GET request
/// using the private key of the given actor, which allows to implement secure
/// federation mode.
pub async fn fetch_object_http_signed<T: Clone, Kind: DeserializeOwned, A: Actor>(
    url: &Url,
    data: &Data<T>,
    actor: &A,
) -> Result<Kind, Error> {
    let config = &data.config;
    // dont fetch local objects this way
    debug_assert!(url.domain() != Some(&config.domain));
    config.verify_url_valid(url).await?;
    info!("Fetching remote object {}", url.to_string());

    let counter = data.request_counter.fetch_add(1, Ordering::SeqCst);
    if counter > config.http_fetch_limit {
        return Err(Error::RequestLimit);
    }

    let req = config
        .client
        .get(url.as_str())
        .header("Accept", FEDERATION_CONTENT_TYPE)
        .timeout(config.request_timeout);

    let private_key_pem = actor
        .private_key_pem()
        .ok_or(anyhow!("Actor does not have a private key to sign with"))?;
    let req = sign_request(
        req,
        actor.id(),
        String::new(),
        private_key_pem,
        data.config.http_signature_compat,
    )
    .await?;

    let res = config.client.execute(req).await.map_err(Error::other)?;

    if res.status() == StatusCode::GONE {
        return Err(Error::ObjectDeleted);
    }

    res.json_limited().await
}
