use futures_core::Future;
use std::{
    fmt::{Debug, Display},
    time::Duration,
};
use tracing::*;

#[derive(Clone, Copy, Default)]
pub(crate) struct RetryStrategy {
    /// Amount of time in seconds to back off
    pub backoff: usize,
    /// Amount of times to retry
    pub retries: usize,
    /// If this particular request has already been retried, you can add an offset here to increment the count to start
    pub offset: usize,
    /// Number of seconds to sleep before trying
    pub initial_sleep: usize,
}

/// Retries a future action factory function up to `amount` times with an exponential backoff timer between tries
pub(crate) async fn retry<
    T,
    E: Display + Debug,
    F: Future<Output = Result<T, E>>,
    A: FnMut() -> F,
>(
    mut action: A,
    strategy: RetryStrategy,
) -> Result<T, E> {
    let mut count = strategy.offset;

    // Do an initial sleep if it's called for
    if strategy.initial_sleep > 0 {
        let sleep_dur = Duration::from_secs(strategy.initial_sleep as u64);
        tokio::time::sleep(sleep_dur).await;
    }

    loop {
        match action().await {
            Ok(val) => return Ok(val),
            Err(err) => {
                if count < strategy.retries {
                    count += 1;

                    let sleep_amt = strategy.backoff.pow(count as u32) as u64;
                    let sleep_dur = Duration::from_secs(sleep_amt);
                    warn!("{err:?}.  Sleeping for {sleep_dur:?} and trying again");
                    tokio::time::sleep(sleep_dur).await;
                    continue;
                } else {
                    return Err(err);
                }
            }
        }
    }
}
