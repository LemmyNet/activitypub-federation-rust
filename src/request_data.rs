use crate::InstanceConfig;
use std::{
    ops::Deref,
    sync::{atomic::AtomicI32, Arc},
};

/// Stores context data which is necessary for the library to work.
#[derive(Clone)]
pub struct ApubContext<T> {
    /// Data which the application requires in handlers, such as database connection
    /// or configuration.
    application_data: Arc<T>,
    /// Configuration of this library.
    pub(crate) local_instance: Arc<InstanceConfig>,
}

impl<T: Clone> ApubContext<T> {
    pub fn new(state: T, local_instance: InstanceConfig) -> ApubContext<T> {
        ApubContext {
            application_data: Arc::new(state),
            local_instance: Arc::new(local_instance),
        }
    }
    pub fn local_instance(&self) -> &InstanceConfig {
        self.local_instance.deref()
    }

    /// Create new [RequestData] from this. You should prefer to use a middleware if possible.
    pub fn to_request_data(&self) -> RequestData<T> {
        RequestData {
            apub_context: self.clone(),
            request_counter: AtomicI32::default(),
        }
    }
}

/// Stores data for handling one specific HTTP request. Most importantly this contains a
/// counter for outgoing HTTP requests. This is necessary to prevent denial of service attacks,
/// where an attacker triggers fetching of recursive objects.
///
/// https://www.w3.org/TR/activitypub/#security-recursive-objects
pub struct RequestData<T> {
    pub(crate) apub_context: ApubContext<T>,
    pub(crate) request_counter: AtomicI32,
}

impl<T> RequestData<T> {
    pub fn local_instance(&self) -> &InstanceConfig {
        self.apub_context.local_instance.deref()
    }
}

impl<T: Clone> Deref for ApubContext<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.application_data
    }
}

impl<T: Clone> Deref for RequestData<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.apub_context.application_data
    }
}

#[derive(Clone)]
pub struct ApubMiddleware<T: Clone>(pub(crate) ApubContext<T>);

impl<T: Clone> ApubMiddleware<T> {
    pub fn new(apub_context: ApubContext<T>) -> Self {
        ApubMiddleware(apub_context)
    }
}
