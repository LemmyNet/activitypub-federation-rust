use crate::{
    activity_queue::util::retry,
    error::Error,
    http_signatures::sign_request,
    reqwest_shim::ResponseExt,
    FEDERATION_CONTENT_TYPE,
};
use anyhow::{anyhow, Context};

use http::{header::HeaderName, HeaderMap, HeaderValue};
use httpdate::fmt_http_date;
use reqwest::Request;
use reqwest_middleware::ClientWithMiddleware;
use std::time::{Duration, SystemTime};

use tracing::debug;
use url::Url;

use super::{util::RetryStrategy, RawActivity};

pub(super) async fn sign_and_send(
    raw: &RawActivity,
    client: &ClientWithMiddleware,
    timeout: Duration,
    retry_strategy: RetryStrategy,
    http_signature_compat: bool,
) -> Result<(), anyhow::Error> {
    let request_builder = client
        .post(raw.inbox.to_string())
        .timeout(timeout)
        .headers(generate_request_headers(&raw.inbox)?);
    let request = sign_request(
        request_builder,
        &raw.actor_id,
        raw.activity.clone(),
        raw.private_key.clone(),
        http_signature_compat,
    )
    .await
    .with_context(|| format!("signing activity {raw}"))?;

    retry(
        || {
            send(
                raw,
                client,
                request
                    .try_clone()
                    .expect("The body of the request is not cloneable"),
            )
        },
        retry_strategy,
    )
    .await
}

pub(super) async fn send(
    raw: &RawActivity,
    client: &ClientWithMiddleware,
    request: Request,
) -> Result<(), anyhow::Error> {
    let response = client.execute(request).await;

    match response {
        Ok(o) if o.status().is_success() => {
            debug!("Activity {raw} delivered successfully",);
            Ok(())
        }
        Ok(o) if o.status().is_client_error() => {
            let text = o.text_limited().await.map_err(Error::other)?;
            debug!("Activity {raw} was rejected, aborting: {}", text);
            Ok(())
        }
        Ok(o) => {
            let status = o.status();
            let text = o.text_limited().await.map_err(Error::other)?;
            Err(anyhow!(
                "Activity {raw} failed with status {}: {}",
                status,
                text,
            ))
        }
        Err(e) => Err(anyhow!(
            "Activity {raw} failed with connection failure: {}",
            e
        )),
    }
}

pub(crate) fn generate_request_headers<U: AsRef<str>>(inbox_url: U) -> Result<HeaderMap, Error> {
    let url = Url::parse(inbox_url.as_ref()).map_err(|err| anyhow!("{err}"))?;
    let mut host = url.domain().expect("read inbox domain").to_string();
    if let Some(port) = url.port() {
        host = format!("{}:{}", host, port);
    }

    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("content-type"),
        HeaderValue::from_static(FEDERATION_CONTENT_TYPE),
    );
    headers.insert(
        HeaderName::from_static("host"),
        HeaderValue::from_str(&host).map_err(|err| anyhow!("{err}"))?,
    );
    headers.insert(
        "date",
        HeaderValue::from_str(&fmt_http_date(SystemTime::now())).map_err(|err| anyhow!("{err}"))?,
    );
    Ok(headers)
}
