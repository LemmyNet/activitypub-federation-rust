use futures_core::Future;
use reqwest_middleware::ClientWithMiddleware;
use std::{
    fmt::Debug,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    task::{JoinHandle, JoinSet},
};

use super::{
    util::RetryStrategy,
    worker::{MainWorker, RetryWorker},
    SendActivityTask,
};

/// A simple activity queue which spawns tokio workers to send out requests
/// When creating a queue, it will spawn a task per worker thread
/// Uses an unbounded mpsc queue for communication (i.e, all messages are in memory)
pub(crate) struct RetryQueue {
    // Stats shared between the queue and workers
    stats: Arc<Stats>,
    sender: UnboundedSender<SendActivityTask>,
    sender_task: JoinHandle<()>,
    retry_sender_task: JoinHandle<()>,
}

/// Simple stat counter to show where we're up to with sending messages
/// This is a lock-free way to share things between tasks
/// When reading these values it's possible (but extremely unlikely) to get stale data if a worker task is in the middle of transitioning
#[derive(Default)]
pub(crate) struct Stats {
    pub pending: AtomicUsize,
    pub running: AtomicUsize,
    pub retries: AtomicUsize,
    pub dead_last_hour: AtomicUsize,
    pub completed_last_hour: AtomicUsize,
}

impl Debug for Stats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Activity queue stats: pending: {}, running: {}, retries: {}, dead: {}, complete: {}",
            self.pending.load(Ordering::Relaxed),
            self.running.load(Ordering::Relaxed),
            self.retries.load(Ordering::Relaxed),
            self.dead_last_hour.load(Ordering::Relaxed),
            self.completed_last_hour.load(Ordering::Relaxed)
        )
    }
}

impl RetryQueue {
    pub fn new(
        client: ClientWithMiddleware,
        worker_count: usize,
        retry_count: usize,
        timeout: Duration,
        backoff: usize, // This should be 60 seconds by default or 1 second in tests
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

        let retry_stats = stats.clone();
        let retry_client = client.clone();

        // The "fast path" retry
        // The backoff should be < 5 mins for this to work otherwise signatures may expire
        // This strategy is the one that is used with the *same* signature
        let strategy = RetryStrategy {
            backoff,
            retries: 1,
        };

        let (retry_sender, retry_sender_task) = RetryWorker::new(
            retry_client.clone(),
            timeout,
            retry_stats.clone(),
            retry_count,
            backoff.pow(2),
        );

        let (sender, receiver) = unbounded_channel();

        let sender_stats = stats.clone();

        let main_worker = Arc::new(MainWorker::new(
            client.clone(),
            timeout,
            retry_sender,
            sender_stats.clone(),
            strategy,
        ));

        let sender_task = tokio::spawn(receiver_queue(worker_count, receiver, move |message| {
            let worker = main_worker.clone();
            async move {
                worker.send(message).await;
            }
        }));

        Self {
            stats,
            sender,
            sender_task,
            retry_sender_task,
        }
    }

    pub(super) async fn queue(&self, message: SendActivityTask) -> Result<(), anyhow::Error> {
        self.stats.pending.fetch_add(1, Ordering::Relaxed);
        self.sender.send(message)?;

        Ok(())
    }

    pub(crate) fn get_stats(&self) -> &Stats {
        &self.stats
    }

    #[allow(unused)]
    // Drops all the senders and shuts down the workers
    pub(crate) async fn shutdown(
        self,
        wait_for_retries: bool,
    ) -> Result<Arc<Stats>, anyhow::Error> {
        drop(self.sender);

        self.sender_task.await?;

        if wait_for_retries {
            self.retry_sender_task.await?;
        }

        Ok(self.stats)
    }
}

// Helper function to abstract away the receiver queue task
// This will use a join set to apply backpressure or have it entirely unbounded
async fn receiver_queue<O: Future<Output = ()> + Send + 'static, F: Fn(SendActivityTask) -> O>(
    worker_count: usize,
    mut receiver: UnboundedReceiver<SendActivityTask>,
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
