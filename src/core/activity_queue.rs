use crate::{
    core::signatures::{sign_request, PublicKey},
    traits::ActivityHandler,
    utils::reqwest_shim::ResponseExt,
    Error,
    InstanceSettings,
    LocalInstance,
    APUB_JSON_CONTENT_TYPE,
};
use anyhow::anyhow;
use background_jobs::{
    memory_storage::{ActixTimer, Storage},
    ActixJob,
    Backoff,
    Manager,
    MaxRetries,
    WorkerConfig,
};
use http::{header::HeaderName, HeaderMap, HeaderValue};
use httpdate::fmt_http_date;
use itertools::Itertools;
use reqwest_middleware::ClientWithMiddleware;
use serde::{Deserialize, Serialize};
use std::{
    fmt::Debug,
    future::Future,
    pin::Pin,
    time::{Duration, SystemTime},
};
use tracing::{debug, info, warn};
use url::Url;

/// Send out the given activity to all inboxes, automatically generating the HTTP signatures. By
/// default, sending is done on a background thread, and automatically retried on failure with
/// exponential backoff.
///
/// - `activity`: The activity to be sent, gets converted to json
/// - `public_key`: The sending actor's public key. In fact, we only need the key id for signing
/// - `private_key`: The sending actor's private key for signing HTTP signature
/// - `recipients`: List of actors who should receive the activity. This gets deduplicated, and
///                 local/invalid inbox urls removed
pub async fn send_activity<Activity>(
    activity: Activity,
    public_key: PublicKey,
    private_key: String,
    recipients: Vec<Url>,
    instance: &LocalInstance,
) -> Result<(), <Activity as ActivityHandler>::Error>
where
    Activity: ActivityHandler + Serialize,
    <Activity as ActivityHandler>::Error: From<anyhow::Error> + From<serde_json::Error>,
{
    let activity_id = activity.id();
    let activity_serialized = serde_json::to_string_pretty(&activity)?;
    let inboxes: Vec<Url> = recipients
        .into_iter()
        .unique()
        .filter(|i| !instance.is_local_url(i))
        .collect();

    let activity_queue = &instance.activity_queue;
    for inbox in inboxes {
        if instance.verify_url_valid(&inbox).await.is_err() {
            continue;
        }

        let message = SendActivityTask {
            activity_id: activity_id.clone(),
            inbox,
            activity: activity_serialized.clone(),
            public_key: public_key.clone(),
            private_key: private_key.clone(),
            http_signature_compat: instance.settings.http_signature_compat,
        };
        if instance.settings.debug {
            let res = do_send(message, &instance.client, instance.settings.request_timeout).await;
            // Don't fail on error, as we intentionally do some invalid actions in tests, to verify that
            // they are rejected on the receiving side. These errors shouldn't bubble up to make the API
            // call fail. This matches the behaviour in production.
            if let Err(e) = res {
                warn!("{}", e);
            }
        } else {
            activity_queue.queue::<SendActivityTask>(message).await?;
            let stats = activity_queue.get_stats().await?;
            info!(
        "Activity queue stats: pending: {}, running: {}, dead (this hour): {}, complete (this hour): {}",
        stats.pending,
        stats.running,
        stats.dead.this_hour(),
        stats.complete.this_hour()
      );
            if stats.running as u64 == instance.settings.worker_count {
                warn!("Maximum number of activitypub workers reached. Consider increasing worker count to avoid federation delays");
            }
        }
    }

    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SendActivityTask {
    activity_id: Url,
    inbox: Url,
    activity: String,
    public_key: PublicKey,
    private_key: String,
    http_signature_compat: bool,
}

/// Signs the activity with the sending actor's key, and delivers to the given inbox. Also retries
/// if the delivery failed.
impl ActixJob for SendActivityTask {
    type State = MyState;
    type Future = Pin<Box<dyn Future<Output = Result<(), anyhow::Error>>>>;
    const NAME: &'static str = "SendActivityTask";

    /// We need to retry activity sending in case the target instances is temporarily unreachable.
    /// In this case, the task is stored and resent when the instance is hopefully back up. This
    /// list shows the retry intervals, and which events of the target instance can be covered:
    /// - 60s (one minute, service restart)
    /// - 60min (one hour, instance maintenance)
    /// - 60h (2.5 days, major incident with rebuild from backup)
    /// TODO: make the intervals configurable
    const MAX_RETRIES: MaxRetries = MaxRetries::Count(3);
    const BACKOFF: Backoff = Backoff::Exponential(60);

    fn run(self, state: Self::State) -> Self::Future {
        Box::pin(async move { do_send(self, &state.client, state.timeout).await })
    }
}

async fn do_send(
    task: SendActivityTask,
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
        task.activity.clone(),
        task.public_key.clone(),
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
            info!(
                "Target server {} rejected {}, aborting",
                task.inbox, task.activity_id,
            );
            Ok(())
        }
        Ok(o) => {
            let status = o.status();
            // Limit the status text to 1KB.
            let text = o.text_limited(1024).await.map_err(Error::conv)?;
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

fn generate_request_headers(inbox_url: &Url) -> HeaderMap {
    let mut host = inbox_url.domain().expect("read inbox domain").to_string();
    if let Some(port) = inbox_url.port() {
        host = format!("{}:{}", host, port);
    }

    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("content-type"),
        HeaderValue::from_static(APUB_JSON_CONTENT_TYPE),
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

pub(crate) fn create_activity_queue(
    client: ClientWithMiddleware,
    settings: &InstanceSettings,
) -> Manager {
    // queue is not used in debug mod, so dont create any workers to avoid log spam
    let worker_count = if settings.debug {
        0
    } else {
        settings.worker_count
    };
    let timeout = settings.request_timeout;

    // Configure and start our workers
    WorkerConfig::new_managed(Storage::new(ActixTimer), move |_| MyState {
        client: client.clone(),
        timeout,
    })
    .register::<SendActivityTask>()
    .set_worker_count("default", worker_count)
    .start()
}

#[derive(Clone)]
struct MyState {
    client: ClientWithMiddleware,
    timeout: Duration,
}
