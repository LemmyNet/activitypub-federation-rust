#![doc = include_str!("../README.md")]
#![deny(missing_docs)]

/// Configuration for this library
pub mod config;
/// Contains main library functionality
pub mod core;
/// Error messages returned by this library.
pub mod error;
/// Data structures which help to define federated messages
pub mod protocol;
/// Traits which need to be implemented for federated data types
pub mod traits;
/// Some utility functions
pub mod utils;
/// Resolves identifiers of the form `name@example.com`
pub mod webfinger;

pub use activitystreams_kinds as kinds;

/// Mime type for Activitypub, used for `Accept` and `Content-Type` HTTP headers
pub static APUB_JSON_CONTENT_TYPE: &str = "application/activity+json";
