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
use http::StatusCode;
use httpdate::fmt_http_date;
use itertools::Itertools;
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue},
    Response,
};
use reqwest_middleware::ClientWithMiddleware;
use rsa::{pkcs8::DecodePrivateKey, RsaPrivateKey};
use serde::Serialize;
use std::{
    fmt::{Debug, Display},
    time::{Duration, Instant, SystemTime},
};
use tracing::{debug, warn};
use url::Url;

#[derive(Clone, Debug)]
/// All info needed to sign and send one activity to one inbox. You should generally use
/// [[crate::activity_queue::queue_activity]] unless you want implement your own queue.
pub struct SendActivityTask {
    pub(crate) actor_id: Url,
    pub(crate) activity_id: Url,
    pub(crate) activity: Bytes,
    pub(crate) inbox: Url,
    pub(crate) private_key: RsaPrivateKey,
    pub(crate) http_signature_compat: bool,
}

impl Display for SendActivityTask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} to {}", self.activity_id, self.inbox)
    }
}

impl SendActivityTask {
    /// Prepare an activity for sending
    ///
    /// - `activity`: The activity to be sent, gets converted to json
    /// - `inboxes`: List of remote actor inboxes that should receive the activity. Ignores local actor
    ///              inboxes. Should be built by calling [crate::traits::Actor::shared_inbox_or_inbox]
    ///              for each target actor.
    pub async fn prepare<Activity, Datatype, ActorType>(
        activity: &Activity,
        actor: &ActorType,
        inboxes: Vec<Url>,
        data: &Data<Datatype>,
    ) -> Result<Vec<SendActivityTask>, Error>
    where
        Activity: ActivityHandler + Serialize + Debug,
        Datatype: Clone,
        ActorType: Actor,
    {
        build_tasks(activity, actor, inboxes, data).await
    }

    /// convert a sendactivitydata to a request, signing and sending it
    pub async fn sign_and_send<Datatype: Clone>(&self, data: &Data<Datatype>) -> Result<(), Error> {
        self.sign_and_send_internal(&data.config.client, data.config.request_timeout)
            .await
    }

    pub(crate) async fn sign_and_send_internal(
        &self,
        client: &ClientWithMiddleware,
        timeout: Duration,
    ) -> Result<(), Error> {
        debug!("Sending {} to {}", self.activity_id, self.inbox,);
        let request_builder = client
            .post(self.inbox.to_string())
            .timeout(timeout)
            .headers(generate_request_headers(&self.inbox));
        let request = sign_request(
            request_builder,
            &self.actor_id,
            self.activity.clone(),
            self.private_key.clone(),
            self.http_signature_compat,
        )
        .await?;

        // Send the activity, and log a warning if its too slow.
        let now = Instant::now();
        let response = client.execute(request).await?;
        let elapsed = now.elapsed().as_secs();
        if elapsed > 10 {
            warn!(
                "Sending activity {} to {} took {}s",
                self.activity_id, self.inbox, elapsed
            );
        }
        self.handle_response(response).await
    }

    /// Based on the HTTP status code determines if an activity was delivered successfully. In that case
    /// Ok is returned. Otherwise it returns Err and the activity send should be retried later.
    ///
    /// Equivalent code in mastodon: https://github.com/mastodon/mastodon/blob/v4.2.8/app/helpers/jsonld_helper.rb#L215-L217
    async fn handle_response(&self, response: Response) -> Result<(), Error> {
        match response.status() {
            status if status.is_success() => {
                debug!("Activity {self} delivered successfully");
                Ok(())
            }
            status
                if status.is_client_error()
                    && status != StatusCode::REQUEST_TIMEOUT
                    && status != StatusCode::TOO_MANY_REQUESTS =>
            {
                let text = response.text_limited().await?;
                debug!("Activity {self} was rejected, aborting: {text}");
                Ok(())
            }
            status => {
                let text = response.text_limited().await?;

                Err(Error::Other(format!(
                    "Activity {self} failure with status {status}: {text}",
                )))
            }
        }
    }
}

pub(crate) async fn build_tasks<Activity, Datatype, ActorType>(
    activity: &Activity,
    actor: &ActorType,
    inboxes: Vec<Url>,
    data: &Data<Datatype>,
) -> Result<Vec<SendActivityTask>, Error>
where
    Activity: ActivityHandler + Serialize + Debug,
    Datatype: Clone,
    ActorType: Actor,
{
    let config = &data.config;
    let actor_id = activity.actor();
    let activity_id = activity.id();
    let activity_serialized: Bytes = serde_json::to_vec(activity)
        .map_err(|e| Error::SerializeOutgoingActivity(e, format!("{:?}", activity)))?
        .into();
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
            actor_id: actor_id.clone(),
            activity_id: activity_id.clone(),
            inbox,
            activity: activity_serialized.clone(),
            private_key: private_key.clone(),
            http_signature_compat: config.http_signature_compat,
        })
    })
    .collect()
    .await)
}

pub(crate) async fn get_pkey_cached<ActorType>(
    data: &Data<impl Clone>,
    actor: &ActorType,
) -> Result<RsaPrivateKey, Error>
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
                RsaPrivateKey::from_pkcs8_pem(&private_key_pem).map_err(|err| {
                    Error::Other(format!("Could not create private key from PEM data:{err}"))
                })
            })
            .await
            .map_err(|err| Error::Other(format!("Error joining: {err}")))??;
            std::result::Result::<RsaPrivateKey, Error>::Ok(pkey)
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
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::{config::FederationConfig, http_signatures::generate_actor_keypair};
    use std::{
        sync::{atomic::AtomicUsize, Arc},
        time::Instant,
    };
    use tracing::info;

    // This will periodically send back internal errors to test the retry
    async fn dodgy_handler(headers: HeaderMap, body: Bytes) -> Result<(), StatusCode> {
        debug!("Headers:{:?}", headers);
        debug!("Body len:{}", body.len());
        Ok(())
    }

    async fn test_server() {
        use axum::{routing::post, Router};

        // We should break every now and then ;)
        let state = Arc::new(AtomicUsize::new(0));

        let app = Router::new()
            .route("/", post(dodgy_handler))
            .with_state(state);

        let listener = tokio::net::TcpListener::bind("0.0.0.0:8001").await.unwrap();
        axum::serve(listener, app.into_make_service())
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
            actor_id: "http://localhost:8001".parse().unwrap(),
            activity_id: "http://localhost:8001/activity".parse().unwrap(),
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
            message.clone().sign_and_send(&data).await?;
        }

        info!("Queue Sent: {:?}", start.elapsed());
        Ok(())
    }

    #[tokio::test]
    async fn test_handle_response() {
        let keypair = generate_actor_keypair().unwrap();
        let message = SendActivityTask {
            actor_id: "http://localhost:8001".parse().unwrap(),
            activity_id: "http://localhost:8001/activity".parse().unwrap(),
            activity: "{}".into(),
            inbox: "http://localhost:8001".parse().unwrap(),
            private_key: keypair.private_key().unwrap(),
            http_signature_compat: true,
        };

        let res = |status| {
            http::Response::builder()
                .status(status)
                .body(vec![])
                .unwrap()
                .into()
        };

        assert!(message.handle_response(res(StatusCode::OK)).await.is_ok());
        assert!(message
            .handle_response(res(StatusCode::BAD_REQUEST))
            .await
            .is_ok());

        assert!(message
            .handle_response(res(StatusCode::MOVED_PERMANENTLY))
            .await
            .is_err());
        assert!(message
            .handle_response(res(StatusCode::REQUEST_TIMEOUT))
            .await
            .is_err());
        assert!(message
            .handle_response(res(StatusCode::TOO_MANY_REQUESTS))
            .await
            .is_err());
        assert!(message
            .handle_response(res(StatusCode::INTERNAL_SERVER_ERROR))
            .await
            .is_err());
    }
}
