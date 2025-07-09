//! Configuration for this library, with various federation settings
//!
//! Use [FederationConfig::builder](crate::config::FederationConfig::builder) to initialize it.
//!
//! ```
//! # use activitypub_federation::config::FederationConfig;
//! # tokio::runtime::Runtime::new().unwrap().block_on(async {
//! let settings = FederationConfig::builder()
//!     .domain("example.com")
//!     .app_data(())
//!     .http_fetch_limit(50)
//!     .build().await?;
//! # Ok::<(), anyhow::Error>(())
//! # }).unwrap()
//! ```

use crate::{
    activity_queue::{create_activity_queue, ActivityQueue},
    error::Error,
    http_signatures::sign_request,
    protocol::verification::verify_domains_match,
    traits::{Activity, Actor},
};
use async_trait::async_trait;
use bytes::Bytes;
use derive_builder::Builder;
use dyn_clone::{clone_trait_object, DynClone};
use moka::future::Cache;
use regex::Regex;
use reqwest::{redirect::Policy, Client, Request};
use reqwest_middleware::{ClientWithMiddleware, RequestBuilder};
use rsa::{pkcs8::DecodePrivateKey, RsaPrivateKey};
use serde::de::DeserializeOwned;
use std::{
    net::IpAddr,
    ops::Deref,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
        OnceLock,
    },
    time::Duration,
};
use tokio::net::lookup_host;
use url::Url;

/// Configuration for this library, with various federation related settings
#[derive(Builder, Clone)]
#[builder(build_fn(private, name = "partial_build"))]
pub struct FederationConfig<T: Clone> {
    /// The domain where this federated instance is running
    #[builder(setter(into))]
    pub(crate) domain: String,
    /// Data which the application requires in handlers, such as database connection
    /// or configuration.
    pub(crate) app_data: T,
    /// Maximum number of outgoing HTTP requests per incoming HTTP request. See
    /// [crate::fetch::object_id::ObjectId] for more details.
    #[builder(default = "20")]
    pub(crate) http_fetch_limit: u32,
    #[builder(default = "default_client()")]
    /// HTTP client used for all outgoing requests. When passing a custom client here you should
    /// also disable redirects and set timeouts.
    ///
    /// Middleware can be used to add functionality like log tracing or retry of failed requests.
    /// Redirects are disabled by default, because automatic redirect URLs can't be validated.
    /// Instead a single redirect is handled manually. The default client sets a timeout of 10s
    ///  to avoid excessive resource usage when connecting to dead servers.
    pub(crate) client: ClientWithMiddleware,
    /// Run library in debug mode. This allows usage of http and localhost urls. It also sends
    /// outgoing activities synchronously, not in background thread. This helps to make tests
    /// more consistent. Do not use for production.
    #[builder(default = "false")]
    pub(crate) debug: bool,
    /// Allow HTTP urls even in production mode
    #[builder(default = "self.debug.unwrap_or(false)")]
    pub(crate) allow_http_urls: bool,
    /// Timeout for all HTTP requests. HTTP signatures are valid for 10s, so it makes sense to
    /// use the same as timeout when sending
    #[builder(default = "Duration::from_secs(10)")]
    pub(crate) request_timeout: Duration,
    /// Function used to verify that urls are valid, See [UrlVerifier] for details.
    #[builder(default = "Box::new(DefaultUrlVerifier())")]
    pub(crate) url_verifier: Box<dyn UrlVerifier + Sync>,
    /// Enable to sign HTTP signatures according to draft 10, which does not include (created) and
    /// (expires) fields. This is required for compatibility with some software like Pleroma.
    /// <https://datatracker.ietf.org/doc/html/draft-cavage-http-signatures-10>
    /// <https://git.pleroma.social/pleroma/pleroma/-/issues/2939>
    #[builder(default = "false")]
    pub(crate) http_signature_compat: bool,
    /// Actor Id and private key to use to sign all federated fetch requests.
    /// This can be used to implement secure mode federation.
    /// <https://docs.joinmastodon.org/spec/activitypub/#secure-mode>
    #[builder(default = "None", setter(custom))]
    pub(crate) signed_fetch_actor: Option<Arc<(Url, RsaPrivateKey)>>,
    #[builder(
        default = "Cache::builder().max_capacity(10000).build()",
        setter(custom)
    )]
    pub(crate) actor_pkey_cache: Cache<Url, RsaPrivateKey>,
    /// Queue for sending outgoing activities. Only optional to make builder work, its always
    /// present once constructed.
    #[builder(setter(skip))]
    pub(crate) activity_queue: Option<Arc<ActivityQueue>>,
    /// When sending with activity queue: Number of tasks that can be in-flight concurrently.
    /// Tasks are retried once after a minute, then put into the retry queue.
    /// Setting this count to `0` means that there is no limit to concurrency
    #[builder(default = "0")]
    pub(crate) queue_worker_count: usize,
    /// When sending with activity queue: Number of concurrent tasks that are being retried
    /// in-flight concurrently. Tasks are retried after an hour, then again in 60 hours.
    /// Setting this count to `0` means that there is no limit to concurrency
    #[builder(default = "0")]
    pub(crate) queue_retry_count: usize,
}

pub(crate) fn domain_regex() -> &'static Regex {
    static DOMAIN_REGEX: OnceLock<Regex> = OnceLock::new();
    DOMAIN_REGEX.get_or_init(|| Regex::new(r"^[a-zA-Z0-9.-]*$").expect("compile regex"))
}

impl<T: Clone> FederationConfig<T> {
    /// Returns a new config builder with default values.
    pub fn builder() -> FederationConfigBuilder<T> {
        FederationConfigBuilder::default()
    }

    pub(crate) async fn verify_url_and_domain<A, Datatype>(&self, activity: &A) -> Result<(), Error>
    where
        A: Activity<DataType = Datatype> + DeserializeOwned + Send + 'static,
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

    /// Create new [Data] from this. You should prefer to use a middleware if possible.
    pub fn to_request_data(&self) -> Data<T> {
        Data {
            config: self.clone(),
            request_counter: Default::default(),
        }
    }

    /// Perform some security checks on URLs as mentioned in activitypub spec, and call user-supplied
    /// [`InstanceSettings.verify_url_function`].
    ///
    /// https://www.w3.org/TR/activitypub/#security-considerations
    pub(crate) async fn verify_url_valid(&self, url: &Url) -> Result<(), Error> {
        match url.scheme() {
            "https" => {}
            "http" => {
                if !self.allow_http_urls {
                    return Err(Error::UrlVerificationError(
                        "Http urls are only allowed in debug mode",
                    ));
                }
            }
            _ => return Err(Error::UrlVerificationError("Invalid url scheme")),
        };

        // Urls which use our local domain are not a security risk, no further verification needed
        if self.is_local_url(url) {
            return Ok(());
        }

        let Some(domain) = url.domain() else {
            return Err(Error::UrlVerificationError("Url must have a domain"));
        };
        if !domain_regex().is_match(domain) {
            return Err(Error::UrlVerificationError("Invalid characters in domain"));
        }

        // Extra checks only for production mode
        if !self.debug {
            if url.port().is_some() {
                return Err(Error::UrlVerificationError("Explicit port is not allowed"));
            }

            // Resolve domain and see if it points to private IP
            // TODO: Use is_global() once stabilized
            //       https://doc.rust-lang.org/std/net/enum.IpAddr.html#method.is_global
            let invalid_ip =
                lookup_host((domain.to_owned(), 80))
                    .await?
                    .any(|addr| match addr.ip() {
                        IpAddr::V4(addr) => {
                            addr.is_private()
                                || addr.is_link_local()
                                || addr.is_loopback()
                                || addr.is_multicast()
                        }
                        IpAddr::V6(addr) => {
                            addr.is_loopback()
                        || addr.is_multicast()
                        || ((addr.segments()[0] & 0xfe00) == 0xfc00) // is_unique_local
                        || ((addr.segments()[0] & 0xffc0) == 0xfe80) // is_unicast_link_local
                        }
                    });
            if invalid_ip {
                return Err(Error::UrlVerificationError(
                    "Localhost is only allowed in debug mode",
                ));
            }
        }

        // It is valid but uncommon for domains to end with `.` char. Drop this so it cant be used
        // to bypass domain blocklist. Avoid cloning url in common case.
        if domain.ends_with('.') {
            let mut url = url.clone();
            let domain = &domain[0..domain.len() - 1];
            url.set_host(Some(domain))?;
            self.url_verifier.verify(&url).await?;
        } else {
            self.url_verifier.verify(url).await?;
        }

        Ok(())
    }

    /// Returns true if the url refers to this instance. Handles hostnames like `localhost:8540` for
    /// local debugging.
    pub(crate) fn is_local_url(&self, url: &Url) -> bool {
        match url.host_str() {
            Some(domain) => {
                let domain = if let Some(port) = url.port() {
                    format!("{}:{}", domain, port)
                } else {
                    domain.to_string()
                };
                domain == self.domain
            }
            None => false,
        }
    }

    /// Returns the local domain
    pub fn domain(&self) -> &str {
        &self.domain
    }
}

impl<T: Clone> FederationConfigBuilder<T> {
    /// Sets an actor to use to sign all federated fetch requests
    pub fn signed_fetch_actor<A: Actor>(&mut self, actor: &A) -> &mut Self {
        let private_key_pem = actor
            .private_key_pem()
            .expect("actor does not have a private key to sign with");

        let private_key =
            RsaPrivateKey::from_pkcs8_pem(&private_key_pem).expect("Could not decode PEM data");
        self.signed_fetch_actor = Some(Some(Arc::new((actor.id().clone(), private_key))));
        self
    }

    /// sets the number of parsed actor private keys to keep in memory
    pub fn actor_pkey_cache(&mut self, cache_size: u64) -> &mut Self {
        self.actor_pkey_cache = Some(Cache::builder().max_capacity(cache_size).build());
        self
    }

    /// Constructs a new config instance with the values supplied to builder.
    ///
    /// Values which are not explicitly specified use the defaults. Also initializes the
    /// queue for outgoing activities, which is stored internally in the config struct.
    /// Requires a tokio runtime for the background queue.
    pub async fn build(&mut self) -> Result<FederationConfig<T>, FederationConfigBuilderError> {
        let mut config = self.partial_build()?;
        let queue = create_activity_queue(
            config.client.clone(),
            config.queue_worker_count,
            config.queue_retry_count,
            config.request_timeout,
        );
        config.activity_queue = Some(Arc::new(queue));
        Ok(config)
    }
}

impl<T: Clone> Deref for FederationConfig<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.app_data
    }
}

/// Handler for validating URLs.
///
/// This is used for implementing domain blocklists and similar functionality. It is called
/// with the ID of newly received activities, when fetching remote data from a given URL
/// and before sending an activity to a given inbox URL. If processing for this domain/URL should
/// be aborted, return an error. In case of `Ok(())`, processing continues.
///
/// ```
/// # use async_trait::async_trait;
/// # use url::Url;
/// # use activitypub_federation::config::UrlVerifier;
/// # use activitypub_federation::error::Error;
/// # #[derive(Clone)]
/// # struct DatabaseConnection();
/// # async fn get_blocklist(_: &DatabaseConnection) -> Vec<String> {
/// #     vec![]
/// # }
/// #[derive(Clone)]
/// struct Verifier {
///     db_connection: DatabaseConnection,
/// }
///
/// #[async_trait]
/// impl UrlVerifier for Verifier {
///     async fn verify(&self, url: &Url) -> Result<(), Error> {
///         let blocklist = get_blocklist(&self.db_connection).await;
///         let domain = url.domain().unwrap().to_string();
///         if blocklist.contains(&domain) {
///             Err(Error::Other("Domain is blocked".into()))
///         } else {
///             Ok(())
///         }
///     }
/// }
/// ```
#[async_trait]
pub trait UrlVerifier: DynClone + Send {
    /// Should return Ok iff the given url is valid for processing.
    async fn verify(&self, url: &Url) -> Result<(), Error>;
}

/// Default URL verifier which does nothing.
#[derive(Clone)]
struct DefaultUrlVerifier();

#[async_trait]
impl UrlVerifier for DefaultUrlVerifier {
    async fn verify(&self, _url: &Url) -> Result<(), Error> {
        Ok(())
    }
}

clone_trait_object!(UrlVerifier);

/// Stores data for handling one specific HTTP request.
///
/// It gives acess to the `app_data` which was passed to [FederationConfig::builder].
///
/// Additionally it contains a counter for outgoing HTTP requests. This is necessary to
/// prevent denial of service attacks, where an attacker triggers fetching of recursive objects.
///
/// <https://www.w3.org/TR/activitypub/#security-recursive-objects>
#[derive(Clone)]
pub struct Data<T: Clone> {
    pub(crate) config: FederationConfig<T>,
    pub(crate) request_counter: RequestCounter,
}

impl<T: Clone> Data<T> {
    /// Returns the data which was stored in [FederationConfigBuilder::app_data]
    pub fn app_data(&self) -> &T {
        &self.config.app_data
    }

    /// The domain that was configured in [FederationConfig].
    pub fn domain(&self) -> &str {
        &self.config.domain
    }

    /// Returns a new instance of `Data` with request counter set to 0.
    pub fn reset_request_count(&self) -> Self {
        Data {
            config: self.config.clone(),
            request_counter: Default::default(),
        }
    }
    /// Total number of outgoing HTTP requests made with this data.
    pub fn request_count(&self) -> u32 {
        self.request_counter.0.load(Ordering::Relaxed)
    }

    /// Add HTTP signature to arbitrary request
    pub async fn sign_request(&self, req: RequestBuilder, body: Bytes) -> Result<Request, Error> {
        let (actor_id, private_key_pem) =
            self.config
                .signed_fetch_actor
                .as_deref()
                .ok_or(Error::Other(
                    "config value signed_fetch_actor is none".to_string(),
                ))?;
        sign_request(
            req,
            actor_id,
            body,
            private_key_pem.clone(),
            self.config.http_signature_compat,
        )
        .await
    }
}

impl<T: Clone> Deref for Data<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.config.app_data
    }
}

/// Wrapper to implement `Clone`
#[derive(Default)]
pub(crate) struct RequestCounter(pub(crate) AtomicU32);

impl Clone for RequestCounter {
    fn clone(&self) -> Self {
        RequestCounter(self.0.load(Ordering::Relaxed).into())
    }
}

/// Middleware for HTTP handlers which provides access to [Data]
#[derive(Clone)]
pub struct FederationMiddleware<T: Clone>(pub(crate) FederationConfig<T>);

impl<T: Clone> FederationMiddleware<T> {
    /// Construct a new middleware instance
    pub fn new(config: FederationConfig<T>) -> Self {
        FederationMiddleware(config)
    }
}

fn default_client() -> ClientWithMiddleware {
    let timeout = Duration::from_secs(10);
    Client::builder()
        .redirect(Policy::none())
        .timeout(timeout)
        .connect_timeout(timeout)
        .build()
        .unwrap_or_else(|_| Client::default())
        .into()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod test {
    use super::*;

    async fn config() -> FederationConfig<i32> {
        FederationConfig::builder()
            .domain("example.com")
            .app_data(1)
            .build()
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn test_url_is_local() -> Result<(), Error> {
        let config = config().await;
        assert!(config.is_local_url(&Url::parse("http://example.com")?));
        assert!(!config.is_local_url(&Url::parse("http://other.com")?));
        // ensure that missing domain doesnt cause crash
        assert!(!config.is_local_url(&Url::parse("http://127.0.0.1")?));
        Ok(())
    }

    #[tokio::test]
    async fn test_get_domain() {
        let config = config().await;
        assert_eq!("example.com", config.domain());
    }
}
