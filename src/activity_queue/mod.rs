//! Queue for signing and sending outgoing activities with retry
//!
#![doc = include_str!("../../docs/09_sending_activities.md")]

use self::{request::sign_and_send, retry_queue::RetryQueue};
use crate::{
    config::Data,
    traits::{ActivityHandler, Actor},
};
use anyhow::anyhow;
use bytes::Bytes;
use itertools::Itertools;
use openssl::pkey::{PKey, Private};
use reqwest_middleware::ClientWithMiddleware;
use serde::Serialize;
use std::{fmt::Debug, sync::atomic::Ordering, time::Duration};
use tracing::{debug, info, warn};
use url::Url;

pub(crate) mod request;
pub(crate) mod retry_queue;
pub(super) mod util;
pub(super) mod worker;

/// Send a new activity to the given inboxes
///
/// - `activity`: The activity to be sent, gets converted to json
/// - `private_key`: Private key belonging to the actor who sends the activity, for signing HTTP
///                  signature. Generated with [crate::http_signatures::generate_actor_keypair].
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
    let actor_id = activity.actor();
    let activity_id = activity.id();
    let activity_serialized: Bytes = serde_json::to_vec(&activity)?.into();
    let private_key_pem = actor
        .private_key_pem()
        .ok_or_else(|| anyhow!("Actor {actor_id} does not contain a private key for signing"))?;

    // This is a mostly expensive blocking call, we don't want to tie up other tasks while this is happening
    let private_key = tokio::task::spawn_blocking(move || {
        PKey::private_key_from_pem(private_key_pem.as_bytes())
            .map_err(|err| anyhow!("Could not create private key from PEM data:{err}"))
    })
    .await
    .map_err(|err| anyhow!("Error joining:{err}"))??;

    let inboxes: Vec<Url> = inboxes
        .into_iter()
        .unique()
        .filter(|i| !config.is_local_url(i))
        .collect();
    // This field is only optional to make builder work, its always present at this point
    let activity_queue = config
        .activity_queue
        .as_ref()
        .expect("Config has activity queue");
    for inbox in inboxes {
        if let Err(err) = config.verify_url_valid(&inbox).await {
            debug!("inbox url invalid, skipping: {inbox}: {err}");
            continue;
        }

        let message = SendActivityTask {
            actor_id: actor_id.to_string(),
            activity_id: activity_id.to_string(),
            inbox: inbox.to_string(),
            activity: activity_serialized.clone(),
            private_key: private_key.clone(),
            http_signature_compat: config.http_signature_compat,
        };

        // Don't use the activity queue if this is in debug mode, send and wait directly
        if config.debug {
            if let Err(err) = sign_and_send(
                &message,
                &config.client,
                config.request_timeout,
                Default::default(),
            )
            .await
            {
                warn!("{err}");
            }
        } else {
            activity_queue.queue(message).await?;
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
// NOTE: These ids & the inbox are actually valid Urls but saved as String to reduce the size of this struct in memory
// Make sure if you adjust the `send_activity` method, then they should be valid urls by the time they get to this struct
pub(super) struct SendActivityTask {
    actor_id: String,
    activity_id: String,
    activity: Bytes,
    inbox: String,
    private_key: PKey<Private>,
    http_signature_compat: bool,
}

/// Creates an activity queue using tokio spawned tasks
/// Note: requires a tokio runtime
pub(crate) fn create_activity_queue(
    client: ClientWithMiddleware,
    worker_count: usize,
    retry_count: usize,
    request_timeout: Duration,
) -> RetryQueue {
    RetryQueue::new(client, worker_count, retry_count, request_timeout, 60)
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
            .filter_module("activitypub_federation", LevelFilter::Debug)
            .format_timestamp(Some(env_logger::TimestampPrecision::Millis))
            .init();

        */

        let activity_queue = RetryQueue::new(
            reqwest::Client::default().into(),
            num_workers,
            num_workers,
            Duration::from_secs(1),
            1,
        );

        let keypair = generate_actor_keypair().unwrap();

        let message = SendActivityTask {
            actor_id: "http://localhost:8001".parse().unwrap(),
            activity_id: "http://localhost:8001/activity".parse().unwrap(),
            activity: "{}".into(),
            inbox: "http://localhost:8001".parse().unwrap(),
            private_key: keypair.private_key().unwrap(),
            http_signature_compat: true,
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

        assert_eq!(
            stats.completed_last_hour.load(Ordering::Relaxed),
            num_messages
        );
    }
}
