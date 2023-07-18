use futures_core::Future;
use std::{
    fmt::{Debug, Display},
    time::Duration,
};

use tracing::warn;

#[derive(Clone, Copy, Default)]
pub(super) struct RetryStrategy {
    /// Amount of time in seconds to back off
    pub backoff: usize,
    /// Amount of times to retry
    pub retries: usize,
}

/// Retries a future action factory function up to `amount` times with an exponential backoff timer between tries
pub(super) async fn retry<
    T,
    E: Display + Debug,
    F: Future<Output = Result<T, E>>,
    A: FnMut() -> F,
>(
    mut action: A,
    strategy: RetryStrategy,
) -> Result<T, E> {
    let mut count = 0;

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
