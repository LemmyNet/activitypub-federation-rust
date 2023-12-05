//! Queue for signing and sending outgoing activities with retry
//!
#![doc = include_str!("../docs/09_sending_activities.md")]

use crate::{
    config::Data,
    error::Error,
    http_signatures::sign_request,
    reqwest_shim::ResponseExt,
    traits::{ActivityHandler, Actor},
    FEDERATION_CONTENT_TYPE,
};

use bytes::Bytes;
use futures::StreamExt;
use httpdate::fmt_http_date;
use itertools::Itertools;
use openssl::pkey::{PKey, Private};
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue},
    Request,
};
use reqwest_middleware::ClientWithMiddleware;
use serde::Serialize;
use std::{
    self,
    fmt::{Debug, Display},
    time::{Duration, SystemTime},
};
use tracing::debug;
use url::Url;

#[derive(Clone, Debug)]
/// all info needed to send one activity to one inbox
pub struct SendActivityTask<'a> {
    actor_id: &'a Url,
    activity_id: &'a Url,
    activity: Bytes,
    inbox: Url,
    private_key: PKey<Private>,
    http_signature_compat: bool,
}
impl Display for SendActivityTask<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} to {}", self.activity_id, self.inbox)
    }
}

impl SendActivityTask<'_> {
    /// prepare an activity for sending
    ///
    /// - `activity`: The activity to be sent, gets converted to json
    /// - `inboxes`: List of remote actor inboxes that should receive the activity. Ignores local actor
    ///              inboxes. Should be built by calling [crate::traits::Actor::shared_inbox_or_inbox]
    ///              for each target actor.
    pub async fn prepare<'a, Activity, Datatype, ActorType>(
        activity: &'a Activity,
        actor: &ActorType,
        inboxes: Vec<Url>,
        data: &Data<Datatype>,
    ) -> Result<Vec<SendActivityTask<'a>>, Error>
    where
        Activity: ActivityHandler + Serialize,
        Datatype: Clone,
        ActorType: Actor,
    {
        let config = &data.config;
        let actor_id = activity.actor();
        let activity_id = activity.id();
        let activity_serialized: Bytes = serde_json::to_vec(&activity).map_err(Error::Json)?.into();
        let private_key = get_pkey_cached(data, actor).await?;

        Ok(futures::stream::iter(
            inboxes
                .into_iter()
                .unique()
                .filter(|i| !config.is_local_url(i)),
        )
        .filter_map(|inbox| async {
            if let Err(err) = config.verify_url_valid(&inbox).await {
                debug!("inbox url invalid, skipping: {inbox}: {err}");
                return None;
            };
            Some(SendActivityTask {
                actor_id,
                activity_id,
                inbox,
                activity: activity_serialized.clone(),
                private_key: private_key.clone(),
                http_signature_compat: config.http_signature_compat,
            })
        })
        .collect()
        .await)
    }

    /// convert a sendactivitydata to a request, signing and sending it
    pub async fn sign_and_send<Datatype: Clone>(&self, data: &Data<Datatype>) -> Result<(), Error> {
        let req = self
            .sign(&data.config.client, data.config.request_timeout)
            .await?;
        self.send(&data.config.client, req).await
    }
    async fn sign(
        &self,
        client: &ClientWithMiddleware,
        timeout: Duration,
    ) -> Result<Request, Error> {
        let task = self;
        let request_builder = client
            .post(task.inbox.to_string())
            .timeout(timeout)
            .headers(generate_request_headers(&task.inbox));
        let request = sign_request(
            request_builder,
            task.actor_id,
            task.activity.clone(),
            task.private_key.clone(),
            task.http_signature_compat,
        )
        .await?;
        Ok(request)
    }

    async fn send(&self, client: &ClientWithMiddleware, request: Request) -> Result<(), Error> {
        let response = client.execute(request).await?;

        match response {
            o if o.status().is_success() => {
                debug!("Activity {self} delivered successfully");
                Ok(())
            }
            o if o.status().is_client_error() => {
                let text = o.text_limited().await?;
                debug!("Activity {self} was rejected, aborting: {text}");
                Ok(())
            }
            o => {
                let status = o.status();
                let text = o.text_limited().await?;

                Err(Error::Other(format!(
                    "Activity {self} failure with status {status}: {text}",
                )))
            }
        }
    }
}

async fn get_pkey_cached<ActorType>(
    data: &Data<impl Clone>,
    actor: &ActorType,
) -> Result<PKey<Private>, Error>
where
    ActorType: Actor,
{
    let actor_id = actor.id();
    // PKey is internally like an Arc<>, so cloning is ok
    data.config
        .actor_pkey_cache
        .try_get_with_by_ref(&actor_id, async {
            let private_key_pem = actor.private_key_pem().ok_or_else(|| {
                Error::Other(format!(
                    "Actor {actor_id} does not contain a private key for signing"
                ))
            })?;

            // This is a mostly expensive blocking call, we don't want to tie up other tasks while this is happening
            let pkey = tokio::task::spawn_blocking(move || {
                PKey::private_key_from_pem(private_key_pem.as_bytes()).map_err(|err| {
                    Error::Other(format!("Could not create private key from PEM data:{err}"))
                })
            })
            .await
            .map_err(|err| Error::Other(format!("Error joining: {err}")))??;
            std::result::Result::<PKey<Private>, Error>::Ok(pkey)
        })
        .await
        .map_err(|e| Error::Other(format!("cloned error: {e}")))
}

pub(crate) fn generate_request_headers(inbox_url: &Url) -> HeaderMap {
    let mut host = inbox_url.domain().expect("read inbox domain").to_string();
    if let Some(port) = inbox_url.port() {
        host = format!("{}:{}", host, port);
    }

    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("content-type"),
        HeaderValue::from_static(FEDERATION_CONTENT_TYPE),
    );
    headers.insert(
        HeaderName::from_static("host"),
        HeaderValue::from_str(&host).expect("Hostname is valid"),
    );
    headers.insert(
        "date",
        HeaderValue::from_str(&fmt_http_date(SystemTime::now())).expect("Date is valid"),
    );
    headers
}

#[cfg(test)]
mod tests {
    use axum::extract::State;
    use bytes::Bytes;
    use http::StatusCode;
    use std::{
        sync::{atomic::AtomicUsize, Arc},
        time::Instant,
    };
    use tracing::info;

    use crate::{config::FederationConfig, http_signatures::generate_actor_keypair};

    use super::*;

    #[allow(unused)]
    // This will periodically send back internal errors to test the retry
    async fn dodgy_handler(
        State(state): State<Arc<AtomicUsize>>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Result<(), StatusCode> {
        debug!("Headers:{:?}", headers);
        debug!("Body len:{}", body.len());

        /*if state.fetch_add(1, Ordering::Relaxed) % 20 == 0 {
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }*/
        Ok(())
    }

    async fn test_server() {
        use axum::{routing::post, Router};

        // We should break every now and then ;)
        let state = Arc::new(AtomicUsize::new(0));

        let app = Router::new()
            .route("/", post(dodgy_handler))
            .with_state(state);

        axum::Server::bind(&"0.0.0.0:8001".parse().unwrap())
            .serve(app.into_make_service())
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    // Sends 100 messages
    async fn test_activity_sending() -> anyhow::Result<()> {
        let num_messages: usize = 100;

        tokio::spawn(test_server());

        /*
        // uncomment for debug logs & stats
        use tracing::log::LevelFilter;

        env_logger::builder()
            .filter_level(LevelFilter::Warn)
            .filter_module("activitypub_federation", LevelFilter::Info)
            .format_timestamp(None)
            .init();

        */
        let keypair = generate_actor_keypair().unwrap();

        let message = SendActivityTask {
            actor_id: &"http://localhost:8001".parse().unwrap(),
            activity_id: &"http://localhost:8001/activity".parse().unwrap(),
            activity: "{}".into(),
            inbox: "http://localhost:8001".parse().unwrap(),
            private_key: keypair.private_key().unwrap(),
            http_signature_compat: true,
        };
        let data = FederationConfig::builder()
            .app_data(())
            .domain("localhost")
            .build()
            .await?
            .to_request_data();

        let start = Instant::now();

        for _ in 0..num_messages {
            message.sign_and_send(&data).await?;
        }

        info!("Queue Sent: {:?}", start.elapsed());
        Ok(())
    }
}
