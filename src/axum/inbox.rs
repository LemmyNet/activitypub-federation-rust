//! Handles incoming activities, verifying HTTP signatures and other checks
//!
#![doc = include_str!("../../docs/08_receiving_activities.md")]

use crate::{
    config::Data,
    error::Error,
    http_signatures::verify_signature,
    parse_received_activity,
    traits::{Activity, Actor, Object},
};
use axum::{
    body::Body,
    extract::FromRequest,
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
};
use http::{HeaderMap, Method, Uri};
use serde::de::DeserializeOwned;
use tracing::debug;

/// Handles incoming activities, verifying HTTP signatures and other checks
pub async fn receive_activity<A, ActorT, Datatype>(
    activity_data: ActivityData,
    data: &Data<Datatype>,
) -> Result<(), <A as Activity>::Error>
where
    A: Activity<DataType = Datatype> + DeserializeOwned + Send + 'static,
    ActorT: Object<DataType = Datatype> + Actor + Send + Sync + 'static,
    for<'de2> <ActorT as Object>::Kind: serde::Deserialize<'de2>,
    <A as Activity>::Error: From<Error> + From<<ActorT as Object>::Error>,
    <ActorT as Object>::Error: From<Error>,
    Datatype: Clone,
{
    let (activity, actor) =
        parse_received_activity::<A, ActorT, _>(&activity_data.body, data).await?;

    verify_signature(
        &activity_data.headers,
        &activity_data.method,
        &activity_data.uri,
        actor.public_key_pem(),
    )?;

    debug!("Receiving activity {}", activity.id().to_string());
    activity.verify(data).await?;
    activity.receive(data).await?;
    Ok(())
}

/// Contains all data that is necessary to receive an activity from an HTTP request
#[derive(Debug)]
pub struct ActivityData {
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    body: Vec<u8>,
}

impl<S> FromRequest<S> for ActivityData
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request(req: Request<Body>, _state: &S) -> Result<Self, Self::Rejection> {
        #[allow(unused_mut)]
        let (mut parts, body) = req.into_parts();

        // take the full URI to handle nested routers
        // OriginalUri::from_request_parts has an Infallible error type
        #[cfg(feature = "axum-original-uri")]
        let uri = {
            use axum::extract::{FromRequestParts, OriginalUri};
            OriginalUri::from_request_parts(&mut parts, _state)
                .await
                .expect("infallible")
                .0
        };
        #[cfg(not(feature = "axum-original-uri"))]
        let uri = parts.uri;

        // this wont work if the body is an long running stream
        let bytes = axum::body::to_bytes(body, usize::MAX)
            .await
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response())?;

        Ok(Self {
            headers: parts.headers,
            method: parts.method,
            uri,
            body: bytes.to_vec(),
        })
    }
}

// TODO: copy tests from actix-web inbox and implement for axum as well
