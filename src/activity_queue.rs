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
use anyhow::anyhow;

use futures_core::Future;
use http::{header::HeaderName, HeaderMap, HeaderValue};
use httpdate::fmt_http_date;
use itertools::Itertools;
use reqwest_middleware::ClientWithMiddleware;
use serde::{Deserialize, Serialize};
use std::{
    fmt::{Debug, Display},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, SystemTime},
};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tracing::{debug, info, warn};
use url::Url;

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
    let activity_serialized = serde_json::to_string_pretty(&activity)?;
    let private_key = actor
        .private_key_pem()
        .expect("Actor for sending activity has private key");
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
        if config.verify_url_valid(&inbox).await.is_err() {
            continue;
        }

        let message = SendActivityTask {
            actor_id: actor_id.clone(),
            activity_id: activity_id.clone(),
            inbox,
            activity: activity_serialized.clone(),
            private_key: private_key.clone(),
            http_signature_compat: config.http_signature_compat,
        };
        if config.debug {
            let res = do_send(&message, &config.client, config.request_timeout).await;
            // Don't fail on error, as we intentionally do some invalid actions in tests, to verify that
            // they are rejected on the receiving side. These errors shouldn't bubble up to make the API
            // call fail. This matches the behaviour in production.
            if let Err(e) = res {
                warn!("{}", e);
            }
        } else {
            activity_queue.queue(message).await?;
            let stats = activity_queue.get_stats();
            let running = stats.running.load(Ordering::Relaxed);
            let stats_fmt = format!(
                "Activity queue stats: pending: {}, running: {}, dead: {}, complete: {}",
                stats.pending.load(Ordering::Relaxed),
                running,
                stats.dead.load(Ordering::Relaxed),
                stats.complete.load(Ordering::Relaxed),
            );
            if running == config.worker_count {
                warn!("Reached max number of send activity workers ({}). Consider increasing worker count to avoid federation delays", config.worker_count);
                warn!(stats_fmt);
            } else {
                info!(stats_fmt);
            }
        }
    }

    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SendActivityTask {
    actor_id: Url,
    activity_id: Url,
    activity: String,
    inbox: Url,
    private_key: String,
    http_signature_compat: bool,
}

async fn do_send(
    task: &SendActivityTask,
    client: &ClientWithMiddleware,
    timeout: Duration,
) -> Result<(), anyhow::Error> {
    debug!("Sending {} to {}", task.activity_id, task.inbox);
    let request_builder = client
        .post(task.inbox.to_string())
        .timeout(timeout)
        .headers(generate_request_headers(&task.inbox));
    let request = sign_request(
        request_builder,
        &task.actor_id,
        task.activity.clone(),
        task.private_key.clone(),
        task.http_signature_compat,
    )
    .await?;
    let response = client.execute(request).await;

    match response {
        Ok(o) if o.status().is_success() => {
            info!(
                "Activity {} delivered successfully to {}",
                task.activity_id, task.inbox
            );
            Ok(())
        }
        Ok(o) if o.status().is_client_error() => {
            let text = o.text_limited().await.map_err(Error::other)?;
            info!(
                "Activity {} was rejected by {}, aborting: {}",
                task.activity_id, task.inbox, text,
            );
            Ok(())
        }
        Ok(o) => {
            let status = o.status();
            let text = o.text_limited().await.map_err(Error::other)?;
            Err(anyhow!(
                "Queueing activity {} to {} for retry after failure with status {}: {}",
                task.activity_id,
                task.inbox,
                status,
                text,
            ))
        }
        Err(e) => {
            info!(
                "Unable to connect to {}, aborting task {}: {}",
                task.inbox, task.activity_id, e
            );
            Ok(())
        }
    }
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

/// A simple activity queue which spawns tokio workers to send out requests
/// When creating a queue, it will spawn a task per worker thread
/// Uses an unbounded mpsc queue for communication (i.e, all messages are in memory)
pub(crate) struct ActivityQueue {
    // Our "background" tasks
    senders: Vec<UnboundedSender<SendActivityTask>>,
    // Round robin of the sender list
    last_sender_idx: AtomicUsize,
    // Stats shared between the queue and workers
    stats: Arc<Stats>,
}

/// Simple stat counter to show where we're up to with sending messages
/// This is a lock-free way to share things between tasks
/// When reading these values it's possible (but extremely unlikely) to get stale data if a worker task is in the middle of transitioning
#[derive(Default)]
struct Stats {
    pending: AtomicUsize,
    running: AtomicUsize,
    dead: AtomicUsize,
    complete: AtomicUsize,
}

/// We need to retry activity sending in case the target instances is temporarily unreachable.
/// In this case, the task is stored and resent when the instance is hopefully back up. This
/// list shows the retry intervals, and which events of the target instance can be covered:
/// - 60s (one minute, service restart)
/// - 60min (one hour, instance maintenance)
/// - 60h (2.5 days, major incident with rebuild from backup)
/// TODO: make the intervals configurable
const MAX_RETRIES: usize = 3;
const BACKOFF: usize = 60;

/// A tokio spawned worker which is responsible for submitting requests to federated servers
async fn worker(
    client: ClientWithMiddleware,
    timeout: Duration,
    mut receiver: UnboundedReceiver<SendActivityTask>,
    stats: Arc<Stats>,
) {
    while let Some(message) = receiver.recv().await {
        // Update our counters as we're now "running" and not "pending"
        stats.pending.fetch_sub(1, Ordering::Relaxed);
        stats.running.fetch_add(1, Ordering::Relaxed);

        // This will use the retry helper method below, with an exponential backoff
        // If the task is sleeping, tokio will use work-stealing to keep it busy with something else
        let outcome = retry(|| do_send(&message, &client, timeout), MAX_RETRIES, BACKOFF).await;

        // "Running" has finished, check the outcome
        stats.running.fetch_sub(1, Ordering::Relaxed);

        match outcome {
            Ok(_) => {
                stats.complete.fetch_add(1, Ordering::Relaxed);
            }
            // We might want to do something here
            Err(_err) => {
                stats.dead.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

impl ActivityQueue {
    fn new(client: ClientWithMiddleware, timeout: Duration, worker_count: usize) -> Self {
        // Keep a vec of senders to send our messages to
        let mut senders = Vec::with_capacity(worker_count);

        let stats: Arc<Stats> = Default::default();

        // Spawn our workers
        for _ in 0..worker_count {
            let (sender, receiver) = unbounded_channel();
            tokio::spawn(worker(client.clone(), timeout, receiver, stats.clone()));
            senders.push(sender);
        }

        Self {
            senders,
            last_sender_idx: AtomicUsize::new(0),
            stats,
        }
    }
    async fn queue(&self, message: SendActivityTask) -> Result<(), anyhow::Error> {
        // really basic round-robin to our workers, we just do mod on the len of senders
        let idx_to_send = self.last_sender_idx.fetch_add(1, Ordering::Relaxed) % self.senders.len();

        // Set a queue to pending
        self.stats.pending.fetch_add(1, Ordering::Relaxed);

        // Send to one of our workers
        self.senders[idx_to_send].send(message)?;

        Ok(())
    }

    fn get_stats(&self) -> &Stats {
        &self.stats
    }
}

/// Creates an activity queue using tokio spawned tasks
/// Note: requires a tokio runtime
pub(crate) fn create_activity_queue(
    client: ClientWithMiddleware,
    worker_count: usize,
    request_timeout: Duration,
    debug: bool,
) -> ActivityQueue {
    // queue is not used in debug mod, so dont create any workers to avoid log spam
    let worker_count = if debug { 0 } else { worker_count };

    ActivityQueue::new(client, request_timeout, worker_count)
}

/// Retries a future action factory function up to `amount` times with an exponential backoff timer between tries
async fn retry<T, E: Display, F: Future<Output = Result<T, E>>, A: FnMut() -> F>(
    mut action: A,
    amount: usize,
    sleep_seconds: usize,
) -> Result<T, E> {
    let mut count = 0;

    loop {
        match action().await {
            Ok(val) => return Ok(val),
            Err(err) => {
                if count < amount {
                    count += 1;

                    warn!("{err}");
                    let sleep_amt = sleep_seconds.pow(count as u32) as u64;
                    tokio::time::sleep(Duration::from_secs(sleep_amt)).await;
                    continue;
                } else {
                    return Err(err);
                }
            }
        }
    }
}
