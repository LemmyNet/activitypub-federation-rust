use super::{
    retry_worker::{RetryRawActivity, RetryWorker},
    RawActivity,
};
use reqwest_middleware::ClientWithMiddleware;
use std::{
    fmt::Debug,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::{sync::mpsc::UnboundedSender, task::JoinHandle};

/// A simple activity queue which spawns tokio workers to send out requests
/// Uses an unbounded mpsc queue for communication (i.e, all messages are in memory)
pub(crate) struct ActivityQueue {
    // Stats shared between the queue and workers
    stats: Arc<Stats>,
    sender: UnboundedSender<RetryRawActivity>,
    sender_task: JoinHandle<()>,
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

impl ActivityQueue {
    pub fn new(
        client: ClientWithMiddleware,
        worker_count: usize,
        retry_count: usize,
        disable_retry: bool,
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

        // Setup & spawn the retry worker
        let (sender, sender_task) = RetryWorker::spawn(
            client,
            timeout,
            stats.clone(),
            worker_count,
            if disable_retry {
                None
            } else {
                Some(retry_count)
            },
            backoff,
            http_signature_compat,
        );

        Self {
            stats,
            sender,
            sender_task,
        }
    }

    pub(super) async fn queue(&self, raw: RawActivity) -> Result<(), anyhow::Error> {
        self.stats.pending.fetch_add(1, Ordering::Relaxed);

        self.sender.send(RetryRawActivity {
            message: raw,
            last_sent: Instant::now(),
            count: 1,
        })?;

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

        Ok(self.stats)
    }
}
