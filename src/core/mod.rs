/// Queue for sending outgoing activities
pub mod activity_queue;
/// Everything related to creation and verification of HTTP signatures, used to authenticate activities
pub mod http_signatures;
/// Typed wrapper for Activitypub Object ID which helps with dereferencing and caching
pub mod object_id;

/// Utilities for using this library with axum web framework
#[cfg(feature = "axum")]
pub mod axum;

/// Utilities for using this library with actix-web framework
#[cfg(feature = "actix-web")]
pub mod actix_web;
