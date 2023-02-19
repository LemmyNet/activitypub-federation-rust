//#![deny(missing_docs)]

pub use activitystreams_kinds as kinds;
pub mod config;
pub mod core;
pub mod protocol;
pub mod request_data;
pub mod traits;
pub mod utils;

/// Mime type for Activitypub, used for `Accept` and `Content-Type` HTTP headers
pub static APUB_JSON_CONTENT_TYPE: &str = "application/activity+json";

/// Error messages returned by this library.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Object was not found in local database")]
    NotFound,
    #[error("Request limit was reached during fetch")]
    RequestLimit,
    #[error("Response body limit was reached during fetch")]
    ResponseBodyLimit,
    #[error("Object to be fetched was deleted")]
    ObjectDeleted,
    #[error("{0}")]
    UrlVerificationError(&'static str),
    #[error("Incoming activity has invalid digest for body")]
    ActivityBodyDigestInvalid,
    #[error("incoming activity has invalid signature")]
    ActivitySignatureInvalid,
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

impl PartialEq for Error {
    fn eq(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}
