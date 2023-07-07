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

/// Checks whether the request is signed by an actor of type A, and returns
/// the actor in question if a valid signature is found.
pub async fn signing_actor<A>(
    request: &HttpRequest,
    body: Option<Bytes>,
    data: &Data<<A as Object>::DataType, <A as Object>::QueueType>,
) -> Result<A, <A as Object>::Error>
where
    A: Object + Actor,
    <A as Object>::Error: From<Error> + From<anyhow::Error>,
    for<'de2> <A as Object>::Kind: Deserialize<'de2>,
{
    verify_body_hash(request.headers().get("Digest"), &body.unwrap_or_default())?;

    http_signatures::signing_actor(request.headers(), request.method(), request.uri(), data).await
}
