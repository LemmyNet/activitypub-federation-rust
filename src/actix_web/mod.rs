//! Utilities for using this library with actix-web framework

pub mod inbox;
#[doc(hidden)]
pub mod middleware;

use crate::{
    config::Data,
    error::Error,
    http_signatures::{self, verify_body_hash},
    traits::{Actor, Object},
};
use actix_web::{web::Bytes, HttpRequest};
use serde::Deserialize;
use std::str::FromStr;

/// Checks whether the request is signed by an actor of type A, and returns
/// the actor in question if a valid signature is found.
pub async fn signing_actor<A>(
    request: &HttpRequest,
    body: Option<Bytes>,
    data: &Data<<A as Object>::DataType>,
) -> Result<A, <A as Object>::Error>
where
    A: Object + Actor,
    <A as Object>::Error: From<Error>,
    for<'de2> <A as Object>::Kind: Deserialize<'de2>,
{
    let header_value = request
        .headers()
        .get("Digest")
        .map(|v| reqwest::header::HeaderValue::from_str(v.to_str().unwrap_or_default()))
        .and_then(std::result::Result::ok);
    verify_body_hash(header_value.as_ref(), &body.unwrap_or_default())?;

    let mut vec = Vec::<(_, _)>::with_capacity(request.headers().len());
    request.headers().iter().for_each(|(k, v)| {
        let k = reqwest::header::HeaderName::from_str(k.as_str()).expect("Failed to parse header");
        let v = reqwest::header::HeaderValue::from_str(v.to_str().unwrap_or_default())
            .expect("Failed to parse header");
        vec.push((k, v));
    });
    let headers = vec.iter().map(|(k, v)| (k, v)).collect::<Vec<(_, _)>>();

    http_signatures::signing_actor(
        headers,
        &reqwest::Method::from_str(request.method().as_str())
            .map_err(|err| Error::Other(err.to_string()))?,
        &http::Uri::from_str(&request.uri().to_string())
            .map_err(|err| Error::Other(err.to_string()))?,
        data,
    )
    .await
}
