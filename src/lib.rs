#![doc = include_str!("../docs/01_intro.md")]
#![doc = include_str!("../docs/02_overview.md")]
#![doc = include_str!("../docs/03_federating_users.md")]
#![doc = include_str!("../docs/04_federating_posts.md")]
#![doc = include_str!("../docs/05_configuration.md")]
#![doc = include_str!("../docs/06_http_endpoints_axum.md")]
#![doc = include_str!("../docs/07_fetching_data.md")]
#![doc = include_str!("../docs/08_receiving_activities.md")]
#![doc = include_str!("../docs/09_sending_activities.md")]
#![doc = include_str!("../docs/10_fetching_objects_with_unknown_type.md")]
#![deny(missing_docs)]

pub mod activity_queue;
pub mod activity_sending;
#[cfg(feature = "actix-web")]
pub mod actix_web;
#[cfg(feature = "axum")]
pub mod axum;
pub mod config;
pub mod error;
pub mod fetch;
pub mod http_signatures;
pub mod protocol;
pub(crate) mod reqwest_shim;
pub mod traits;
pub mod url;

use crate::{
    config::Data,
    error::Error,
    fetch::object_id::ObjectId,
    traits::{ActivityHandler, Actor, Object},
};
pub use activitystreams_kinds as kinds;

use crate::url::Url;
use serde::{de::DeserializeOwned, Deserialize};

/// Mime type for Activitypub data, used for `Accept` and `Content-Type` HTTP headers
pub const FEDERATION_CONTENT_TYPE: &str = "application/activity+json";

/// Deserialize incoming inbox activity to the given type, perform basic
/// validation and extract the actor.
async fn parse_received_activity<Activity, ActorT, Datatype>(
    body: &[u8],
    data: &Data<Datatype>,
) -> Result<(Activity, ActorT), <Activity as ActivityHandler>::Error>
where
    Activity: ActivityHandler<DataType = Datatype> + DeserializeOwned + Send + 'static,
    ActorT: Object<DataType = Datatype> + Actor + Send + 'static,
    for<'de2> <ActorT as Object>::Kind: serde::Deserialize<'de2>,
    <Activity as ActivityHandler>::Error: From<Error> + From<<ActorT as Object>::Error>,
    <ActorT as Object>::Error: From<Error>,
    Datatype: Clone,
{
    let activity: Activity = serde_json::from_slice(body).map_err(|e| {
        // Attempt to include activity id in error message
        let id = extract_id(body).ok();
        Error::ParseReceivedActivity(e, id)
    })?;
    data.config.verify_url_and_domain(&activity).await?;
    let actor = ObjectId::<ActorT>::from(activity.actor().clone())
        .dereference(data)
        .await?;
    Ok((activity, actor))
}

/// Attempt to parse id field from serialized json
fn extract_id(data: &[u8]) -> serde_json::Result<Url> {
    #[derive(Deserialize)]
    struct Id {
        id: Url,
    }
    Ok(serde_json::from_slice::<Id>(data)?.id)
}
