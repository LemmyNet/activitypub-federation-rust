use reqwest_middleware::ClientWithMiddleware;

use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tokio::sync::mpsc::UnboundedSender;
use tracing::warn;

use super::{
    request::sign_and_send,
    util::{retry, RetryStrategy},
    SendActivityTask,
    Stats,
};

/// A tokio spawned worker which is responsible for submitting requests to federated servers
/// This will retry up to one time with the same signature, and if it fails, will move it to the retry queue.
/// We need to retry activity sending in case the target instances is temporarily unreachable.
/// In this case, the task is stored and resent when the instance is hopefully back up. This
/// list shows the retry intervals, and which events of the target instance can be covered:
/// - 60s (one minute, service restart) -- happens in the worker w/ same signature
/// - 60min (one hour, instance maintenance) --- happens in the retry worker
/// - 60h (2.5 days, major incident with rebuild from backup) --- happens in the retry worker
pub(super) async fn activity_worker(
    client: ClientWithMiddleware,
    timeout: Duration,
    message: SendActivityTask,
    retry_queue: UnboundedSender<SendActivityTask>,
    stats: Arc<Stats>,
    strategy: RetryStrategy,
    http_signature_compat: bool,
) {
    stats.pending.fetch_sub(1, Ordering::Relaxed);
    stats.running.fetch_add(1, Ordering::Relaxed);

    let outcome = sign_and_send(&message, &client, timeout, strategy, http_signature_compat).await;

    // "Running" has finished, check the outcome
    stats.running.fetch_sub(1, Ordering::Relaxed);

    match outcome {
        Ok(_) => {
            stats.completed_last_hour.fetch_add(1, Ordering::Relaxed);
        }
        Err(_err) => {
            stats.retries.fetch_add(1, Ordering::Relaxed);
            warn!(
                "Sending activity {} to {} to the retry queue to be tried again later",
                message.activity_id, message.inbox
            );
            // Send to the retry queue.  Ignoring whether it succeeds or not
            retry_queue.send(message).ok();
        }
    }
}

pub(super) async fn retry_worker(
    client: ClientWithMiddleware,
    timeout: Duration,
    message: SendActivityTask,
    stats: Arc<Stats>,
    strategy: RetryStrategy,
    http_signature_compat: bool,
) {
    // Because the times are pretty extravagant between retries, we have to re-sign each time
    let outcome = retry(
        || {
            sign_and_send(
                &message,
                &client,
                timeout,
                RetryStrategy {
                    backoff: 0,
                    retries: 0,
                    offset: 0,
                    initial_sleep: 0,
                },
                http_signature_compat,
            )
        },
        strategy,
    )
    .await;

    stats.retries.fetch_sub(1, Ordering::Relaxed);

    match outcome {
        Ok(_) => {
            stats.completed_last_hour.fetch_add(1, Ordering::Relaxed);
        }
        Err(_err) => {
            stats.dead_last_hour.fetch_add(1, Ordering::Relaxed);
        }
    }
}
