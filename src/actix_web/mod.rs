//! Utilities for using this library with actix-web framework

mod http_compat;
pub mod inbox;
#[doc(hidden)]
pub mod middleware;
pub mod response;

use crate::{
    config::Data,
    error::Error,
    http_signatures::{self, verify_body_hash},
    traits::{Actor, Object},
};
use actix_web::{web::Bytes, HttpRequest};
use serde::Deserialize;

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
    let digest_header = request
        .headers()
        .get("Digest")
        .map(http_compat::header_value);
    verify_body_hash(digest_header.as_ref(), &body.unwrap_or_default())?;

    let headers = http_compat::header_map(request.headers());
    let method = http_compat::method(request.method());
    let uri = http_compat::uri(request.uri());
    http_signatures::signing_actor(&headers, &method, &uri, data).await
}
