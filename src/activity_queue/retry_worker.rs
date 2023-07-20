use super::{queue::Stats, request::sign_and_send, util::RetryStrategy, RawActivity};
use futures_core::Future;
use futures_util::FutureExt;
use reqwest_middleware::ClientWithMiddleware;
use std::{
    collections::{BTreeMap, BinaryHeap},
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};
use tokio::{
    sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender, WeakUnboundedSender},
    task::{JoinHandle, JoinSet},
    time::MissedTickBehavior,
};
use tracing::{error, info};

/// A tokio spawned worker which is responsible for submitting requests to federated servers
/// This will retry up to one time with the same signature, and if it fails, will move it to the retry queue.
/// We need to retry activity sending in case the target instances is temporarily unreachable.
/// In this case, the task is stored and resent when the instance is hopefully back up. This
/// list shows the retry intervals, and which events of the target instance can be covered:
/// - 60s (one minute, service restart) -- happens in the worker w/ same signature
/// - >60min (one hour, instance maintenance) --- happens in the retry worker
/// - >60h (2.5 days, major incident with rebuild from backup) --- happens in the retry worker
pub(super) struct RetryWorker {
    client: ClientWithMiddleware,
    timeout: Duration,
    stats: Arc<Stats>,
    batch_sender: WeakUnboundedSender<RetryRawActivity>,
    backoff: usize,
    http_signature_compat: bool,
}

/// A message that has tried to be sent but has not been able to be sent
#[derive(Debug, PartialEq, Eq)]
pub(super) struct RetryRawActivity {
    /// The message that is sent
    pub message: RawActivity,
    /// The time this was last sent
    pub last_sent: Instant,
    /// The current count
    pub count: usize,
}

// We reverse the order here as we want the "highest" to be the earliest, not latest
// So that we can retry the oldest sent first
impl Ord for RetryRawActivity {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.last_sent.cmp(&other.last_sent).reverse()
    }
}

impl PartialOrd for RetryRawActivity {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl RetryWorker {
    /// Spawns a background task for managing the queue of retryables
    pub fn spawn(
        client: ClientWithMiddleware,
        timeout: Duration,
        stats: Arc<Stats>,
        worker_count: usize,
        retry_count: Option<usize>,
        backoff: usize,
        http_signature_compat: bool,
    ) -> (UnboundedSender<RetryRawActivity>, JoinHandle<()>) {
        // The main sender channel, gets called immediately when something is queued
        let (sender, receiver) = unbounded_channel::<RetryRawActivity>();
        // The batch sender channel, checks every hour if anything needs to be sent
        let (batch_sender, batch_receiver) = unbounded_channel::<RetryRawActivity>();
        // The retry sender channel, is called by the batch
        let (retry_sender, retry_receiver) = unbounded_channel::<RetryRawActivity>();

        let worker = Arc::new(Self {
            client,
            timeout,
            stats,
            batch_sender: batch_sender.downgrade(),
            backoff,
            http_signature_compat,
        });

        let retry_task = tokio::spawn(async move {
            // This is the main worker queue, tasks sent here are sent immediately
            let main_worker = worker.clone();
            let worker_queue = receiver_queue(worker_count, receiver, move |message| {
                let worker = main_worker.clone();
                async move {
                    worker.send(message).await;
                }
            });

            // If retries are enabled, start up our batch task and retry queue
            if let Some(retry_count) = retry_count {
                // This task checks every hour anything that needs to be sent, based upon the last sent time
                // If any tasks need to be sent, they are then sent to the retry queue
                let batch_loop = retry_loop(backoff.pow(2), batch_receiver, retry_sender);

                let retry_queue = receiver_queue(retry_count, retry_receiver, move |message| {
                    let worker = worker.clone();
                    async move {
                        worker.send(message).await;
                    }
                });

                let wait_for_batch = worker_queue.then(|_| async move {
                    // Wait a little bit before dropping the batch sender for tests
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    drop(batch_sender);
                });

                tokio::join!(wait_for_batch, retry_queue, batch_loop);
            } else {
                drop(batch_sender);
                tokio::join!(worker_queue);
            }
        });

        (sender, retry_task)
    }

    async fn send(&self, mut retry: RetryRawActivity) {
        // If this is the first time running
        if retry.count == 1 {
            self.stats.pending.fetch_sub(1, Ordering::Relaxed);
            self.stats.running.fetch_add(1, Ordering::Relaxed);
        }

        let outcome = sign_and_send(
            &retry.message,
            &self.client,
            self.timeout,
            if retry.count == 1 {
                RetryStrategy {
                    backoff: self.backoff,
                    retries: 1,
                }
            } else {
                Default::default()
            },
            self.http_signature_compat,
        )
        .await;

        if retry.count == 1 {
            self.stats.running.fetch_sub(1, Ordering::Relaxed);
        }
        match outcome {
            Ok(_) => {
                self.stats
                    .completed_last_hour
                    .fetch_add(1, Ordering::Relaxed);
                if retry.count != 1 {
                    self.stats.retries.fetch_sub(1, Ordering::Relaxed);
                }
            }
            Err(_err) => {
                // If retries are enabled
                if let Some(sender) = self.batch_sender.upgrade() {
                    // If this is the first time, we append it to the retry count
                    if retry.count == 1 {
                        self.stats.retries.fetch_add(1, Ordering::Relaxed);
                    }

                    // If this is under 3 retries
                    if retry.count < 3 {
                        retry.count += 1;
                        retry.last_sent = Instant::now();
                        sender.send(retry).ok();
                    } else {
                        self.stats.dead_last_hour.fetch_add(1, Ordering::Relaxed);
                    }
                } else {
                    self.stats.dead_last_hour.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }
}

/// Ordered list of raw activities based upon retry count
///
/// Uses separate binary heaps per count to keep things in order
///
/// When flushed it will go through each queue and check to see if there are any retries ready to be sent
///
/// If enought time has elapsed it'll send them with the sender, otherwise they'll stay in the queue
struct RetryQueue {
    /// Queue per retry count for ordering
    queues: BTreeMap<usize, BinaryHeap<RetryRawActivity>>,
    sender: UnboundedSender<RetryRawActivity>,
    sleep_interval: usize,
}

impl RetryQueue {
    /// Push a raw activity onto the queue
    fn push(&mut self, retry: RetryRawActivity) {
        let queue = self.queues.entry(retry.count).or_default();
        queue.push(retry);
    }

    /// Flush out & send any retries that need to be retried
    fn flush(&mut self) {
        let mut count = 0;
        let mut total = 0;

        // We check each queue separately
        for (retry_count, queue) in self.queues.iter_mut() {
            // We check the duration based on the retry count using an exponential backoff, i.e, 60s, 60m, 60h
            let sleep_duration =
                Duration::from_secs(self.sleep_interval.pow(*retry_count as u32) as u64);

            total += queue.len();

            'queue: loop {
                match queue.pop() {
                    Some(retry) => {
                        // If the elapsed time is long enough we send it
                        if retry.last_sent.elapsed() > sleep_duration {
                            if let Err(err) = self.sender.send(retry) {
                                error!("Error sending retry: {err}");
                            }
                            count += 1;
                        // If it's too young, then we exit the loop
                        // No more entries after this will be old enough in the binary heap
                        } else {
                            queue.push(retry);
                            break 'queue;
                        }
                    }
                    None => break 'queue,
                }
            }
        }

        if total > 0 {
            info!("Scheduled {count}/{total} activities for retry");
        }
    }
}

/// This is a retry loop that will simply send tasks in batches
/// It will check an incoming queue, and schedule any tasks that need to be sent
/// The current sleep interval here is 1 hour
async fn retry_loop(
    sleep_interval: usize,
    mut batch_receiver: UnboundedReceiver<RetryRawActivity>,
    retry_sender: UnboundedSender<RetryRawActivity>,
) {
    let mut interval = tokio::time::interval(Duration::from_secs((sleep_interval) as u64));
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let mut inner = RetryQueue {
        queues: Default::default(),
        sender: retry_sender,
        sleep_interval,
    };

    loop {
        tokio::select! {
            message = batch_receiver.recv() => {
                match message {
                    // We have a new message, add it to our queue
                    Some(retry) => {
                        inner.push(retry);
                    },
                    // The receiver has dropped, so flush out everything and then exit the loop
                    None => {
                        inner.flush();
                        break;
                    }
                }
            }
            _ = interval.tick() => {
                inner.flush();
            }
        }
    }
}

/// Helper function to abstract away the receiver queue task.
///
/// This will use a join set to apply backpressure or have it entirely unbounded if the worker_count is 0
pub(super) async fn receiver_queue<O: Future<Output = ()> + Send + 'static, F: Fn(A) -> O, A>(
    worker_count: usize,
    mut receiver: UnboundedReceiver<A>,
    spawn_fn: F,
) {
    // If we're above the worker count, we create a joinset to apply a bit of backpressure here
    if worker_count > 0 {
        let mut join_set = JoinSet::new();

        while let Some(message) = receiver.recv().await {
            // If we're over the limit of workers, wait for them to finish before spawning
            while join_set.len() >= worker_count {
                join_set.join_next().await;
            }

            let task = spawn_fn(message);

            join_set.spawn(task);
        }

        // Drain the queue if we receive no extra messages
        while !join_set.is_empty() {
            join_set.join_next().await;
        }
    } else {
        // If the worker count is `0` then just spawn and don't use the join_set
        while let Some(message) = receiver.recv().await {
            let task = spawn_fn(message);
            tokio::spawn(task);
        }
    }
}
