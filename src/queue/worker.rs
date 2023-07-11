//! Worker for sending activity messages
use anyhow::Error;
use async_trait::async_trait;
use derive_builder::Builder;
use reqwest_middleware::ClientWithMiddleware;

use std::time::Duration;

use crate::config::FederationConfig;

use super::{request::sign_and_send, util::RetryStrategy, ActivityMessage, ActivityQueue};
/// Configuration for the worker, with various tweaks
#[derive(Builder, Clone)]
pub struct Worker {
    #[builder(default = "reqwest::Client::default().into()")]
    /// The Reqwest client to make requests with
    pub client: ClientWithMiddleware,
    #[builder(default = "Duration::from_secs(10)")]
    /// The timeout before a request fails
    pub request_timeout: Duration,
    #[builder(default)]
    /// The strategy for retrying requests
    pub strategy: RetryStrategy,
    #[builder(default = "false")]
    /// Whether to enable signature compat or not
    pub http_signature_compat: bool,
}

impl Worker {
    /// Create a worker from a config
    pub fn from_config<T: Clone>(config: &FederationConfig<T>) -> Self {
        Self {
            client: config.client.clone(),
            request_timeout: config.request_timeout,
            strategy: Default::default(),
            http_signature_compat: config.http_signature_compat,
        }
    }
}

#[async_trait]
impl ActivityQueue for Worker {
    type Error = anyhow::Error;

    async fn queue(&self, message: ActivityMessage) -> Result<(), Error> {
        sign_and_send(
            &message,
            &self.client,
            self.request_timeout,
            self.strategy,
            self.http_signature_compat,
        )
        .await
    }
}
