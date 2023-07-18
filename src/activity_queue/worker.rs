use futures::{stream::FuturesUnordered, StreamExt};
use reqwest_middleware::ClientWithMiddleware;
use std::{
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};
use tokio::{
    sync::mpsc::{error::TryRecvError, unbounded_channel, UnboundedSender, WeakUnboundedSender},
    task::JoinHandle,
};
use tracing::{debug, error, info};

use super::{request::sign_and_send, retry_queue::Stats, util::RetryStrategy, SendActivityTask};
/// A tokio spawned worker which is responsible for submitting requests to federated servers
/// This will retry up to one time with the same signature, and if it fails, will move it to the retry queue.
/// We need to retry activity sending in case the target instances is temporarily unreachable.
/// In this case, the task is stored and resent when the instance is hopefully back up. This
/// list shows the retry intervals, and which events of the target instance can be covered:
/// - 60s (one minute, service restart) -- happens in the worker w/ same signature
/// - 60min (one hour, instance maintenance) --- happens in the retry worker
/// - 60h (2.5 days, major incident with rebuild from backup) --- happens in the retry worker
pub(super) struct MainWorker {
    pub client: ClientWithMiddleware,
    pub timeout: Duration,
    pub retry_queue: UnboundedSender<SendRetryTask>,
    pub stats: Arc<Stats>,
    pub strategy: RetryStrategy,
}

impl MainWorker {
    pub fn new(
        client: ClientWithMiddleware,
        timeout: Duration,
        retry_queue: UnboundedSender<SendRetryTask>,
        stats: Arc<Stats>,
        strategy: RetryStrategy,
    ) -> Self {
        Self {
            client,
            timeout,
            retry_queue,
            stats,
            strategy,
        }
    }
    pub async fn send(&self, message: SendActivityTask) {
        self.stats.pending.fetch_sub(1, Ordering::Relaxed);
        self.stats.running.fetch_add(1, Ordering::Relaxed);

        let outcome = sign_and_send(&message, &self.client, self.timeout, self.strategy).await;

        // "Running" has finished, check the outcome
        self.stats.running.fetch_sub(1, Ordering::Relaxed);

        match outcome {
            Ok(_) => {
                self.stats
                    .completed_last_hour
                    .fetch_add(1, Ordering::Relaxed);
            }
            Err(_err) => {
                self.stats.retries.fetch_add(1, Ordering::Relaxed);
                debug!(
                    "Sending activity {} to {} to the retry queue to be tried again later",
                    message.activity_id, message.inbox
                );
                // Send to the retry queue.  Ignoring whether it succeeds or not
                if let Err(err) = self.retry_queue.send(SendRetryTask {
                    message,
                    last_sent: Instant::now(),
                    count: 2,
                }) {
                    error!("Error sending to retry queue:{err}");
                }
            }
        }
    }
}

// this is a retry worker that will basically keep a list of pending futures, and try at regular intervals to send them all in a batch
pub(super) struct RetryWorker {
    client: ClientWithMiddleware,
    timeout: Duration,
    stats: Arc<Stats>,
    retry_sender: WeakUnboundedSender<SendRetryTask>,
}

pub(super) struct SendRetryTask {
    message: SendActivityTask,
    // The time this was last sent
    last_sent: Instant,
    // The current count
    count: usize,
}

impl RetryWorker {
    pub fn new(
        client: ClientWithMiddleware,
        timeout: Duration,
        stats: Arc<Stats>,
        max_workers: usize,
        sleep_interval: usize,
    ) -> (UnboundedSender<SendRetryTask>, JoinHandle<()>) {
        let (retry_sender, mut retry_receiver) = unbounded_channel::<SendRetryTask>();

        let worker = Self {
            client,
            timeout,
            stats,
            retry_sender: retry_sender.clone().downgrade(),
        };

        let join_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs((sleep_interval) as u64));

            loop {
                interval.tick().await;
                // A list of pending futures
                let futures = FuturesUnordered::new();
                let now = Instant::now();

                let mut requeue_messages = Vec::new();

                // Grab all the activities that are waiting to be sent
                loop {
                    // try_recv will not await anything
                    match retry_receiver.try_recv() {
                        Ok(message) => {
                            let sleep_duration = Duration::from_secs(
                                sleep_interval.pow(message.count as u32) as u64,
                                // Take off 1 second for tests to pass
                            ) - Duration::from_secs(1);

                            // If the time between now and sending this message is greater than our sleep duration
                            if now - message.last_sent > sleep_duration {
                                futures.push(worker.send(message));
                            } else {
                                // If we haven't slept long enough, then we just add it to the end of the queue
                                requeue_messages.push(message);
                            }

                            // If we have reached our max concurrency count
                            if max_workers > 0 && futures.len() >= max_workers {
                                break;
                            }
                        }
                        Err(TryRecvError::Empty) => {
                            // no more to be had, break and wait for the next interval
                            break;
                        }
                        Err(TryRecvError::Disconnected) => {
                            // The queue has completely disconnected, and so we should shut down this task
                            // Drain the queue then exit after
                            futures.collect::<()>().await;
                            return;
                        }
                    }
                }

                if futures.len() > 0 || requeue_messages.len() > 0 {
                    // Drain the queue
                    info!(
                        "Retrying {}/{} messages",
                        futures.len(),
                        requeue_messages.len() + futures.len()
                    );
                }

                futures.collect::<()>().await;

                for message in requeue_messages {
                    worker
                        .retry_sender
                        .upgrade()
                        .and_then(|sender| sender.send(message).ok());
                }
            }
        });

        (retry_sender, join_handle)
    }

    async fn send(&self, mut retry: SendRetryTask) {
        // Because the times are pretty extravagant between retries, we have to re-sign each time
        let outcome = sign_and_send(
            &retry.message,
            &self.client,
            self.timeout,
            RetryStrategy {
                backoff: 0,
                retries: 0,
            },
        )
        .await;

        self.stats.retries.fetch_sub(1, Ordering::Relaxed);

        match outcome {
            Ok(_) => {
                self.stats
                    .completed_last_hour
                    .fetch_add(1, Ordering::Relaxed);
            }
            Err(_err) => {
                if retry.count < 3 {
                    retry.count += 1;
                    retry.last_sent = Instant::now();
                    self.retry_sender
                        .upgrade()
                        .and_then(|sender| sender.send(retry).ok());
                } else {
                    self.stats.dead_last_hour.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }
}
