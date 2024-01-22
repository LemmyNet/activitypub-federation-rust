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
    verify_body_hash(request.headers().get("Digest"), &body.unwrap_or_default())?;

    http_signatures::signing_actor(
        request.headers(),
        request.method(),
        &http::Uri::from_str(&request.uri().to_string()).unwrap(),
        data,
    )
    .await
}
