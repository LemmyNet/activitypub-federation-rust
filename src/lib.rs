use crate::{
    core::activity_queue::create_activity_queue,
    traits::ActivityHandler,
    utils::verify_domains_match,
};
use async_trait::async_trait;
use background_jobs::Manager;
use derive_builder::Builder;
use dyn_clone::{clone_trait_object, DynClone};
use reqwest_middleware::ClientWithMiddleware;
use serde::de::DeserializeOwned;
use std::time::Duration;
use url::Url;

pub mod core;
pub mod data;
pub mod deser;
pub mod traits;
pub mod utils;

/// Mime type for Activitypub, used for `Accept` and `Content-Type` HTTP headers
pub static APUB_JSON_CONTENT_TYPE: &str = "application/activity+json";

/// Represents a single, federated instance (for example lemmy.ml). There should only be one of
/// this in your application (except for testing).
pub struct LocalInstance {
    hostname: String,
    client: ClientWithMiddleware,
    activity_queue: Manager,
    settings: InstanceSettings,
}

impl LocalInstance {
    async fn verify_url_and_domain<Activity, Datatype>(
        &self,
        activity: &Activity,
    ) -> Result<(), Error>
    where
        Activity: ActivityHandler<DataType = Datatype> + DeserializeOwned + Send + 'static,
    {
        verify_domains_match(activity.id(), activity.actor())?;
        self.verify_url_valid(activity.id()).await?;
        if self.is_local_url(activity.id()) {
            return Err(Error::UrlVerificationError(
                "Activity was sent from local instance",
            ));
        }

        Ok(())
    }

    /// Perform some security checks on URLs as mentioned in activitypub spec, and call user-supplied
    /// [`InstanceSettings.verify_url_function`].
    ///
    /// https://www.w3.org/TR/activitypub/#security-considerations
    async fn verify_url_valid(&self, url: &Url) -> Result<(), Error> {
        match url.scheme() {
            "https" => {}
            "http" => {
                if !self.settings.debug {
                    return Err(Error::UrlVerificationError(
                        "Http urls are only allowed in debug mode",
                    ));
                }
            }
            _ => return Err(Error::UrlVerificationError("Invalid url scheme")),
        };

        if url.domain().is_none() {
            return Err(Error::UrlVerificationError("Url must have a domain"));
        }

        if url.domain() == Some("localhost") && !self.settings.debug {
            return Err(Error::UrlVerificationError(
                "Localhost is only allowed in debug mode",
            ));
        }

        self.settings
            .url_verifier
            .verify(url)
            .await
            .map_err(Error::UrlVerificationError)?;

        Ok(())
    }
}

#[async_trait]
pub trait UrlVerifier: DynClone + Send {
    async fn verify(&self, url: &Url) -> Result<(), &'static str>;
}
clone_trait_object!(UrlVerifier);

// Use InstanceSettingsBuilder to initialize this
#[derive(Builder)]
pub struct InstanceSettings {
    /// Maximum number of outgoing HTTP requests per incoming activity
    #[builder(default = "20")]
    http_fetch_retry_limit: i32,
    /// Number of worker threads for sending outgoing activities
    #[builder(default = "64")]
    worker_count: u64,
    /// Run library in debug mode. This allows usage of http and localhost urls. It also sends
    /// outgoing activities synchronously, not in background thread. This helps to make tests
    /// more consistent.
    /// Do not use for production.
    #[builder(default = "false")]
    debug: bool,
    /// Timeout for all HTTP requests. HTTP signatures are valid for 10s, so it makes sense to
    /// use the same as timeout when sending
    #[builder(default = "Duration::from_secs(10)")]
    request_timeout: Duration,
    /// Function used to verify that urls are valid, used when receiving activities or fetching remote
    /// objects. Use this to implement functionality like federation blocklists. In case verification
    /// fails, it should return an error message.
    #[builder(default = "Box::new(DefaultUrlVerifier())")]
    url_verifier: Box<dyn UrlVerifier + Sync>,
    /// Enable to sign HTTP signatures according to draft 10, which does not include (created) and
    /// (expires) fields. This is required for compatibility with some software like Pleroma.
    /// https://datatracker.ietf.org/doc/html/draft-cavage-http-signatures-10
    /// https://git.pleroma.social/pleroma/pleroma/-/issues/2939
    #[builder(default = "false")]
    http_signature_compat: bool,
}

impl InstanceSettings {
    /// Returns a new settings builder.
    pub fn builder() -> InstanceSettingsBuilder {
        <_>::default()
    }
}

#[derive(Clone)]
struct DefaultUrlVerifier();

#[async_trait]
impl UrlVerifier for DefaultUrlVerifier {
    async fn verify(&self, _url: &Url) -> Result<(), &'static str> {
        Ok(())
    }
}

impl LocalInstance {
    pub fn new(domain: String, client: ClientWithMiddleware, settings: InstanceSettings) -> Self {
        let activity_queue = create_activity_queue(client.clone(), &settings);
        LocalInstance {
            hostname: domain,
            client,
            activity_queue,
            settings,
        }
    }
    /// Returns true if the url refers to this instance. Handles hostnames like `localhost:8540` for
    /// local debugging.
    fn is_local_url(&self, url: &Url) -> bool {
        let mut domain = url.domain().expect("id has domain").to_string();
        if let Some(port) = url.port() {
            domain = format!("{}:{}", domain, port);
        }
        domain == self.hostname
    }

    /// Returns the local hostname
    pub fn hostname(&self) -> &str {
        &self.hostname
    }
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Object was not found in database")]
    NotFound,
    #[error("Request limit was reached during fetch")]
    RequestLimit,
    #[error("Object to be fetched was deleted")]
    ObjectDeleted,
    #[error("{0}")]
    UrlVerificationError(&'static str),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl Error {
    pub fn conv<T>(error: T) -> Self
    where
        T: Into<anyhow::Error>,
    {
        Error::Other(error.into())
    }
}
