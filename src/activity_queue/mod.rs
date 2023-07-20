//! Queue for signing and sending outgoing activities with retry
//!
#![doc = include_str!("../../docs/09_sending_activities.md")]

use self::{queue::ActivityQueue, request::sign_and_send};
use crate::{
    config::Data,
    traits::{ActivityHandler, Actor},
};
use anyhow::anyhow;
use bytes::Bytes;
use futures_util::StreamExt;
use itertools::Itertools;
use openssl::pkey::{PKey, Private};
use reqwest_middleware::ClientWithMiddleware;
use serde::Serialize;
use std::{
    fmt::{Debug, Display},
    sync::atomic::Ordering,
    time::Duration,
};
use tracing::{debug, info, warn};
use url::Url;

pub(crate) mod queue;
pub(crate) mod request;
pub(super) mod retry_worker;
pub(super) mod util;

/// Send a new activity to the given inboxes
///
/// - `activity`: The activity to be sent, gets converted to json
/// - `actor`: The actor doing the sending
/// - `inboxes`: List of remote actor inboxes that should receive the activity. Ignores local actor
///              inboxes. Should be built by calling [crate::traits::Actor::shared_inbox_or_inbox]
///              for each target actor.
pub async fn send_activity<Activity, Datatype, ActorType>(
    activity: Activity,
    actor: &ActorType,
    inboxes: Vec<Url>,
    data: &Data<Datatype>,
) -> Result<(), <Activity as ActivityHandler>::Error>
where
    Activity: ActivityHandler + Serialize,
    <Activity as ActivityHandler>::Error: From<anyhow::Error> + From<serde_json::Error>,
    Datatype: Clone,
    ActorType: Actor,
{
    let config = &data.config;

    // This field is only optional to make builder work, its always present at this point
    let activity_queue = config
        .activity_queue
        .as_ref()
        .expect("Config has activity queue");

    for raw_activity in prepare_raw(&activity, actor, inboxes, data).await? {
        // Don't use the activity queue if this is in debug mode, send and wait directly
        if config.debug {
            if let Err(err) = sign_and_send(
                &raw_activity,
                &config.client,
                config.request_timeout,
                Default::default(),
                config.http_signature_compat,
            )
            .await
            {
                warn!("{err}");
            }
        } else {
            activity_queue.queue(raw_activity).await?;
            let stats = activity_queue.get_stats();
            let running = stats.running.load(Ordering::Relaxed);
            if running == config.worker_count && config.worker_count != 0 {
                warn!("Reached max number of send activity workers ({}). Consider increasing worker count to avoid federation delays", config.worker_count);
                warn!("{:?}", stats);
            } else {
                info!("{:?}", stats);
            }
        }
    }

    Ok(())
}

#[derive(Clone, Debug)]
/// A raw opaque activity to an inbox that can be sent directly rather than via the queue
// NOTE: These ids & the inbox are actually valid Urls but saved as String to reduce the size of this struct in memory
pub struct RawActivity {
    actor_id: String,
    activity_id: String,
    activity: Bytes,
    inbox: String,
    private_key: PKey<Private>,
}

impl PartialEq for RawActivity {
    fn eq(&self, other: &Self) -> bool {
        self.actor_id == other.actor_id
            && self.activity_id == other.activity_id
            && self.activity == other.activity
            && self.inbox == other.inbox
    }
}

impl Eq for RawActivity {}

impl RawActivity {
    /// Sends a raw activity directly, rather than using the background queue.
    /// This will sign and send the request using the configured [`client`](crate::config::FederationConfigBuilder::client) in the federation config
    pub async fn send<Datatype: Clone>(
        &self,
        data: &Data<Datatype>,
    ) -> Result<(), crate::error::Error> {
        let config = &data.config;
        Ok(sign_and_send(
            self,
            &config.client,
            config.request_timeout,
            Default::default(),
            config.http_signature_compat,
        )
        .await?)
    }
}

impl Display for RawActivity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} to {}", self.activity_id, self.inbox)
    }
}

/// Prepare a list of raw activities for sending to individual inboxes.
///
/// All inboxes are checked to see if they are valid non-local urls.
///
/// If you want to send activities to a background queue, use [`send_activity`]
/// Once prepared, you can use the [`RawActivity::send`] method to send the activity to an inbox
pub async fn prepare_raw<Activity, Datatype, ActorType>(
    activity: &Activity,
    actor: &ActorType,
    inboxes: Vec<Url>,
    data: &Data<Datatype>,
) -> Result<Vec<RawActivity>, <Activity as ActivityHandler>::Error>
where
    Activity: ActivityHandler + Serialize,
    <Activity as ActivityHandler>::Error: From<anyhow::Error> + From<serde_json::Error>,
    Datatype: Clone,
    ActorType: Actor,
{
    let config = &data.config;
    let actor_id = activity.actor();
    let activity_id = activity.id();
    let activity_serialized: Bytes = serde_json::to_vec(&activity)?.into();
    let private_key_pem = actor
        .private_key_pem()
        .ok_or_else(|| anyhow!("Actor {actor_id} does not contain a private key for signing"))?;

    let private_key = tokio::task::spawn_blocking(move || {
        PKey::private_key_from_pem(private_key_pem.as_bytes())
            .map_err(|err| anyhow!("Could not create private key from PEM data:{err}"))
    })
    .await
    .map_err(|err| anyhow!("Error joining:{err}"))??;

    Ok(futures_util::stream::iter(
        inboxes
            .into_iter()
            .unique()
            .filter(|i| !config.is_local_url(i)),
    )
    .filter_map(|inbox| async {
        let inbox = inbox;
        if let Err(err) = config.verify_url_valid(&inbox).await {
            debug!("inbox url invalid, skipping: {inbox}: {err}");
            return None;
        };
        Some(RawActivity {
            actor_id: actor_id.to_string(),
            activity_id: activity_id.to_string(),
            inbox: inbox.to_string(),
            activity: activity_serialized.clone(),
            private_key: private_key.clone(),
        })
    })
    .collect()
    .await)
}

/// Creates an activity queue using tokio spawned tasks
/// Note: requires a tokio runtime
pub(crate) fn create_activity_queue(
    client: ClientWithMiddleware,
    worker_count: usize,
    retry_count: usize,
    disable_retry: bool,
    request_timeout: Duration,
    http_signature_compat: bool,
) -> ActivityQueue {
    ActivityQueue::new(
        client,
        worker_count,
        retry_count,
        disable_retry,
        request_timeout,
        60,
        http_signature_compat,
    )
}

#[cfg(test)]
mod tests {
    use axum::extract::State;
    use bytes::Bytes;
    use http::{HeaderMap, StatusCode};
    use std::{
        sync::{atomic::AtomicUsize, Arc},
        time::Instant,
    };
    use tracing::trace;

    use crate::http_signatures::generate_actor_keypair;

    use super::*;

    // This will periodically send back internal errors to test the retry
    async fn dodgy_handler(
        State(state): State<Arc<AtomicUsize>>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Result<(), StatusCode> {
        trace!("Headers:{:?}", headers);
        trace!("Body len:{}", body.len());

        if state.fetch_add(1, Ordering::Relaxed) % 20 == 0 {
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
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
    // Queues 100 messages and then asserts that the worker runs them
    async fn test_activity_queue_workers() {
        let num_workers = 64;
        let num_messages: usize = 100;

        tokio::spawn(test_server());

        /*
        // uncomment for debug logs & stats
        use tracing::log::LevelFilter;

        env_logger::builder()
            .filter_level(LevelFilter::Warn)
            .filter_module("activitypub_federation", LevelFilter::Info)
            .format_timestamp(Some(env_logger::TimestampPrecision::Millis))
            .init();
        */

        let activity_queue = ActivityQueue::new(
            reqwest::Client::default().into(),
            num_workers,
            num_workers,
            false,
            Duration::from_secs(1),
            1,
            true,
        );

        let keypair = generate_actor_keypair().unwrap();

        let message = RawActivity {
            actor_id: "http://localhost:8001".into(),
            activity_id: "http://localhost:8001/activity".into(),
            activity: "{}".into(),
            inbox: "http://localhost:8001".into(),
            private_key: keypair.private_key().unwrap(),
        };

        let start = Instant::now();

        for _ in 0..num_messages {
            activity_queue.queue(message.clone()).await.unwrap();
        }

        info!("Queue Sent: {:?}", start.elapsed());

        let stats = activity_queue.shutdown(true).await.unwrap();

        info!(
            "Queue Finished.  Num msgs: {}, Time {:?}, msg/s: {:0.0}",
            num_messages,
            start.elapsed(),
            num_messages as f64 / start.elapsed().as_secs_f64()
        );

        info!("Stats: {:?}", stats);
        assert_eq!(
            stats.completed_last_hour.load(Ordering::Relaxed),
            num_messages
        );
    }
}
