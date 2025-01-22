//! Handles incoming activities, verifying HTTP signatures and other checks
//!
#![doc = include_str!("../../docs/08_receiving_activities.md")]

use crate::{
    config::Data,
    error::Error,
    http_signatures::verify_signature,
    parse_received_activity,
    traits::{ActivityHandler, Actor, Object},
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
pub async fn receive_activity<Activity, ActorT, Datatype>(
    activity_data: ActivityData,
    data: &Data<Datatype>,
) -> Result<(), <Activity as ActivityHandler>::Error>
where
    Activity: ActivityHandler<DataType = Datatype> + DeserializeOwned + Send + 'static,
    ActorT: Object<DataType = Datatype> + Actor + Send + 'static,
    for<'de2> <ActorT as Object>::Kind: serde::Deserialize<'de2>,
    <Activity as ActivityHandler>::Error: From<Error> + From<<ActorT as Object>::Error>,
    <ActorT as Object>::Error: From<Error>,
    Datatype: Clone,
{
    let (activity, actor) =
        parse_received_activity::<Activity, ActorT, _>(&activity_data.body, data).await?;

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
        let (parts, body) = req.into_parts();

        // this wont work if the body is an long running stream
        let bytes = axum::body::to_bytes(body, usize::MAX)
            .await
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response())?;

        Ok(Self {
            headers: parts.headers,
            method: parts.method,
            uri: parts.uri,
            body: bytes.to_vec(),
        })
    }
}

// TODO: copy tests from actix-web inbox and implement for axum as well
