use crate::{
    core::{object_id::ObjectId, signatures::verify_signature},
    request_data::RequestData,
    traits::{ActivityHandler, Actor, ApubObject},
    Error,
};
use actix_web::{HttpRequest, HttpResponse};
use serde::de::DeserializeOwned;
use tracing::debug;

/// Receive an activity and perform some basic checks, including HTTP signature verification.
pub async fn receive_activity<Activity, ActorT, Datatype>(
    request: HttpRequest,
    activity: Activity,
    data: &RequestData<Datatype>,
) -> Result<HttpResponse, <Activity as ActivityHandler>::Error>
where
    Activity: ActivityHandler<DataType = Datatype> + DeserializeOwned + Send + 'static,
    ActorT: ApubObject<DataType = Datatype> + Actor + Send + 'static,
    for<'de2> <ActorT as ApubObject>::ApubType: serde::Deserialize<'de2>,
    <Activity as ActivityHandler>::Error: From<anyhow::Error>
        + From<Error>
        + From<<ActorT as ApubObject>::Error>
        + From<serde_json::Error>,
    <ActorT as ApubObject>::Error: From<Error> + From<anyhow::Error>,
{
    data.local_instance()
        .verify_url_and_domain(&activity)
        .await?;

    let actor = ObjectId::<ActorT>::new(activity.actor().clone())
        .dereference(data)
        .await?;

    verify_signature(
        request.headers(),
        request.method(),
        request.uri(),
        actor.public_key(),
    )?;

    debug!("Receiving activity {}", activity.id().to_string());
    activity.receive(data).await?;
    Ok(HttpResponse::Ok().finish())
}
