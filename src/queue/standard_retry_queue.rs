//! A simple activity queue which spawns tokio workers to send out requests
//! Uses an unbounded mpsc queue for communication (i.e, all messages are in memory)
use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};

use async_trait::async_trait;
use reqwest_middleware::ClientWithMiddleware;
use tokio::{
    sync::mpsc::{unbounded_channel, UnboundedSender},
    task::{JoinHandle, JoinSet},
};
use tracing::{info, warn};

use crate::config::FederationConfig;

use super::{
    util::{retry, RetryStrategy},
    worker::Worker,
    ActivityMessage,
    ActivityQueue,
    Stats,
};
/// A tokio spawned worker queue which is responsible for submitting requests to federated servers
/// This will retry up to one time with the same signature, and if it fails, will move it to the retry queue.
/// We need to retry activity sending in case the target instances is temporarily unreachable.
/// In this case, the task is stored and resent when the instance is hopefully back up. This
/// list shows the retry intervals, and which events of the target instance can be covered:
/// - 60s (one minute, service restart) -- happens in the worker w/ same signature
/// - 60min (one hour, instance maintenance) --- happens in the retry queue
/// - 60h (2.5 days, major incident with rebuild from backup) --- happens in the retry queue
pub struct StandardRetryQueue {
    // Stats shared between the queue and workers
    stats: Arc<Stats>,
    worker_count: usize,
    sender: UnboundedSender<ActivityMessage>,
    sender_task: JoinHandle<()>,
    retry_sender_task: JoinHandle<()>,
}

#[async_trait]
impl ActivityQueue for StandardRetryQueue {
    type Error = anyhow::Error;

    async fn queue(&self, message: ActivityMessage) -> Result<(), Self::Error> {
        self.stats.pending.fetch_add(1, Ordering::Relaxed);
        self.sender.send(message)?;

        let running = self.stats.running.load(Ordering::Relaxed);
        if running == self.worker_count && self.worker_count != 0 {
            warn!("Reached max number of send activity workers ({}). Consider increasing worker count to avoid federation delays", self.worker_count);
            warn!("{:?}", self.stats);
        } else {
            info!("{:?}", self.stats);
        }

        Ok(())
    }
}

impl StandardRetryQueue {
    /// Builds a simple queue from a federation config
    pub fn from_config<T: Clone>(config: &FederationConfig<T>) -> Self {
        Self::new(
            config.client.clone(),
            config.worker_count,
            config.retry_count,
            config.request_timeout,
            60,
            config.http_signature_compat,
        )
    }

    /// Construct a new queue
    pub fn new(
        client: ClientWithMiddleware,
        worker_count: usize,
        retry_count: usize,
        timeout: Duration,
        backoff: usize, // This should be 60 seconds by default or 1 second in tests
        http_signature_compat: bool,
    ) -> Self {
        let stats: Arc<Stats> = Default::default();

        // This task clears the dead/completed stats every hour
        let hour_stats = stats.clone();
        tokio::spawn(async move {
            let duration = Duration::from_secs(3600);
            loop {
                tokio::time::sleep(duration).await;
                hour_stats.completed_last_hour.store(0, Ordering::Relaxed);
                hour_stats.dead_last_hour.store(0, Ordering::Relaxed);
            }
        });

        let (retry_sender, mut retry_receiver) = unbounded_channel();
        let retry_stats = stats.clone();

        // The "fast path" retry
        // The backoff should be < 5 mins for this to work otherwise signatures may expire
        // This strategy is the one that is used with the *same* signature
        let strategy = RetryStrategy {
            backoff,
            retries: 1,
            offset: 0,
            initial_sleep: 0,
        };

        let worker = Arc::new(Worker {
            client: client.clone(),
            request_timeout: timeout,
            strategy,
            http_signature_compat,
        });

        // The "retry path" strategy
        // After the fast path fails, a task will sleep up to backoff ^ 2 and then retry again
        let retry_strategy = RetryStrategy {
            backoff,
            retries: 3,
            offset: 2,
            initial_sleep: backoff.pow(2), // wait 60 mins before even trying
        };

        let retry_worker = Arc::new(Worker {
            client: client.clone(),
            request_timeout: timeout,
            // Internally we need to re-sign the message each attempt so we remove this strategy
            strategy: RetryStrategy::default(),
            http_signature_compat,
        });

        let retry_sender_task = tokio::spawn(async move {
            let mut join_set = JoinSet::new();

            while let Some(message) = retry_receiver.recv().await {
                let retry_task = retry_task(
                    retry_stats.clone(),
                    retry_worker.clone(),
                    message,
                    retry_strategy,
                );

                if retry_count > 0 {
                    // If we're over the limit of retries, wait for them to finish before spawning
                    while join_set.len() >= retry_count {
                        join_set.join_next().await;
                    }

                    join_set.spawn(retry_task);
                } else {
                    // If the retry worker count is `0` then just spawn and don't use the join_set
                    tokio::spawn(retry_task);
                }
            }

            while !join_set.is_empty() {
                join_set.join_next().await;
            }
        });

        let (sender, mut receiver) = unbounded_channel();

        let sender_stats = stats.clone();

        let sender_task = tokio::spawn(async move {
            let mut join_set = JoinSet::new();

            while let Some(message) = receiver.recv().await {
                let task = main_task(
                    sender_stats.clone(),
                    worker.clone(),
                    message,
                    retry_sender.clone(),
                );

                if worker_count > 0 {
                    // If we're over the limit of workers, wait for them to finish before spawning
                    while join_set.len() >= worker_count {
                        join_set.join_next().await;
                    }

                    join_set.spawn(task);
                } else {
                    // If the worker count is `0` then just spawn and don't use the join_set
                    tokio::spawn(task);
                }
            }

            drop(retry_sender);

            while !join_set.is_empty() {
                join_set.join_next().await;
            }
        });

        Self {
            stats,
            sender,
            sender_task,
            retry_sender_task,
            worker_count,
        }
    }

    /// Drops all the senders and shuts down the workers
    pub async fn shutdown(self, wait_for_retries: bool) -> Result<Arc<Stats>, anyhow::Error> {
        drop(self.sender);

        self.sender_task.await?;

        if wait_for_retries {
            self.retry_sender_task.await?;
        }

        Ok(self.stats)
    }
}

pub(super) async fn main_task(
    stats: Arc<Stats>,
    worker: Arc<Worker>,
    message: ActivityMessage,
    retry_queue: UnboundedSender<ActivityMessage>,
) {
    stats.pending.fetch_sub(1, Ordering::Relaxed);
    stats.running.fetch_add(1, Ordering::Relaxed);

    let outcome = worker.queue(message.clone()).await;

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

pub(super) async fn retry_task(
    stats: Arc<Stats>,
    worker: Arc<Worker>,
    message: ActivityMessage,
    strategy: RetryStrategy,
) {
    // Because the times are pretty extravagant between retries, we have to re-sign each time
    let outcome = retry(|| worker.queue(message.clone()), strategy).await;

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
