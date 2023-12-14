//! Error messages returned by this library

use std::string::FromUtf8Error;

use http_signature_normalization_reqwest::SignError;
use openssl::error::ErrorStack;
use url::Url;

use crate::fetch::webfinger::WebFingerError;

/// Error messages returned by this library
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// Object was not found in local database
    #[error("Object was not found in local database")]
    NotFound,
    /// Request limit was reached during fetch
    #[error("Request limit was reached during fetch")]
    RequestLimit,
    /// Response body limit was reached during fetch
    #[error("Response body limit was reached during fetch")]
    ResponseBodyLimit,
    /// Object to be fetched was deleted
    #[error("Fetched remote object {0} which was deleted")]
    ObjectDeleted(Url),
    /// url verification error
    #[error("URL failed verification: {0}")]
    UrlVerificationError(&'static str),
    /// Incoming activity has invalid digest for body
    #[error("Incoming activity has invalid digest for body")]
    ActivityBodyDigestInvalid,
    /// Incoming activity has invalid signature
    #[error("Incoming activity has invalid signature")]
    ActivitySignatureInvalid,
    /// Failed to resolve actor via webfinger
    #[error("Failed to resolve actor via webfinger")]
    WebfingerResolveFailed(#[from] WebFingerError),
    /// JSON Error
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Failed to parse an activity received from another instance
    #[error("Failed to parse incoming activity with id {0}: {1}")]
    ParseReceivedActivity(Url, serde_json::Error),
    /// Reqwest Middleware Error
    #[error(transparent)]
    ReqwestMiddleware(#[from] reqwest_middleware::Error),
    /// Reqwest Error
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
    /// UTF-8 error
    #[error(transparent)]
    Utf8(#[from] FromUtf8Error),
    /// Url Parse
    #[error(transparent)]
    UrlParse(#[from] url::ParseError),
    /// Signing errors
    #[error(transparent)]
    SignError(#[from] SignError),
    /// Other generic errors
    #[error("{0}")]
    Other(String),
}

impl From<ErrorStack> for Error {
    fn from(value: ErrorStack) -> Self {
        Error::Other(value.to_string())
    }
}

impl PartialEq for Error {
    fn eq(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}
