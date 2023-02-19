use crate::config::FederationConfig;
use std::{ops::Deref, sync::atomic::AtomicI32};

/// Stores data for handling one specific HTTP request.
///
/// Most importantly this contains a counter for outgoing HTTP requests. This is necessary to
/// prevent denial of service attacks, where an attacker triggers fetching of recursive objects.
///
/// <https://www.w3.org/TR/activitypub/#security-recursive-objects>
pub struct RequestData<T: Clone> {
    pub(crate) config: FederationConfig<T>,
    pub(crate) request_counter: AtomicI32,
}

impl<T: Clone> RequestData<T> {
    pub fn app_data(&self) -> &T {
        &self.config.app_data
    }
    pub fn hostname(&self) -> &str {
        &self.config.hostname
    }
}

impl<T: Clone> Deref for RequestData<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.config.app_data
    }
}

#[derive(Clone)]
pub struct ApubMiddleware<T: Clone>(pub(crate) FederationConfig<T>);

impl<T: Clone> ApubMiddleware<T> {
    pub fn new(config: FederationConfig<T>) -> Self {
        ApubMiddleware(config)
    }
}
