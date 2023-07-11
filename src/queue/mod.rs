//! Queue for signing and sending outgoing activities with retry
//!
#![doc = include_str!("../../docs/09_sending_activities.md")]

pub(crate) mod request;
pub mod standard_retry_queue;
pub mod util;
pub mod worker;

use crate::{
    config::Data,
    traits::{ActivityHandler, Actor},
};
use anyhow::anyhow;

use async_trait::async_trait;
use bytes::Bytes;

use itertools::Itertools;
use openssl::pkey::{PKey, Private};
use serde::Serialize;
use std::{
    fmt::Debug,
    sync::atomic::{AtomicUsize, Ordering},
};
use uuid::Uuid;

use tracing::{debug, warn};
use url::Url;

use self::request::sign_and_send;

#[async_trait]
/// Anything that can enqueue outgoing activitypub requests
pub trait ActivityQueue {
    /// The errors that can be returned when queuing
    type Error: Debug;

    /// Queues one activity task to a specific inbox
    async fn queue(&self, message: ActivityMessage) -> Result<(), Self::Error>;
}

/// Sends an activity with an outbound activity queue
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
    let queue = data
        .config
        .activity_queue
        .clone()
        .expect("Activity Queue is always configured");
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

    for inbox in inboxes {
        if let Err(err) = config.verify_url_valid(&inbox).await {
            debug!("inbox url invalid, skipping: {inbox}: {err}");
            continue;
        }

        let message = ActivityMessage {
            id: Uuid::new_v4(),
            actor_id: actor_id.clone(),
            activity_id: activity_id.clone(),
            inbox,
            activity: activity_serialized.clone(),
            private_key: private_key.clone(),
        };

        // Don't use the activity queue if this is in debug mode, send and wait directly
        if config.debug {
            if let Err(err) = sign_and_send(
                &message,
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
            queue
                .send(message)
                .await
                .map_err(|err| anyhow!("Error sending to queue:{err:?}"))?;
            //let stats = queue.stats();
            //let running = stats.running.load(Ordering::Relaxed);
            //if running == config.worker_count && config.worker_count != 0 {
            //    warn!("Reached max number of send activity workers ({}). Consider increasing worker count to avoid federation delays", config.worker_count);
            //    warn!("{:?}", stats);
            //} else {
            //    info!("{:?}", stats);
            //}
        }
    }

    Ok(())
}

#[derive(Clone, Debug)]
/// The struct sent to a worker for processing
/// When `send_activity` is used, it is split up to tasks per-inbox
pub struct ActivityMessage {
    /// The ID of the activity task
    pub id: Uuid,
    /// The actor ID
    pub actor_id: Url,
    /// The activity ID
    pub activity_id: Url,
    /// The activity body in JSON
    pub activity: Bytes,
    /// The inbox to send the activity to
    pub inbox: Url,
    /// The private key to sign the request
    pub private_key: PKey<Private>,
}

/// Simple stat counter to show where we're up to with sending messages
/// This is a lock-free way to share things between tasks
/// When reading these values it's possible (but extremely unlikely) to get stale data if a worker task is in the middle of transitioning
#[derive(Default)]
pub struct Stats {
    pending: AtomicUsize,
    running: AtomicUsize,
    retries: AtomicUsize,
    dead_last_hour: AtomicUsize,
    completed_last_hour: AtomicUsize,
}

impl Debug for Stats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Activity queue stats: pending: {}, running: {}, retries: {}, dead (last hr): {}, complete (last hr): {}",
            self.pending.load(Ordering::Relaxed),
            self.running.load(Ordering::Relaxed),
            self.retries.load(Ordering::Relaxed),
            self.dead_last_hour.load(Ordering::Relaxed),
            self.completed_last_hour.load(Ordering::Relaxed)
        )
    }
}

#[cfg(test)]
mod tests {
    use axum::extract::State;
    use bytes::Bytes;
    use http::{HeaderMap, StatusCode};
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };

    use crate::{
        http_signatures::generate_actor_keypair,
        queue::standard_retry_queue::StandardRetryQueue,
    };
    use tracing::{debug, info};

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
            .format_timestamp(None)
            .init();

        */

        let queue = StandardRetryQueue::new(
            reqwest::Client::default().into(),
            num_workers,
            num_workers,
            Duration::from_secs(10),
            1,
            true,
        );

        let keypair = generate_actor_keypair().unwrap();

        let message = ActivityMessage {
            id: Uuid::new_v4(),
            actor_id: "http://localhost:8001".parse().unwrap(),
            activity_id: "http://localhost:8001/activity".parse().unwrap(),
            activity: "{}".into(),
            inbox: "http://localhost:8001".parse().unwrap(),
            private_key: keypair.private_key().unwrap(),
        };

        let start = Instant::now();

        for _ in 0..num_messages {
            queue.queue(message.clone()).await.unwrap();
        }

        info!("Queue Sent: {:?}", start.elapsed());

        let stats = queue.shutdown(true).await.unwrap();

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
