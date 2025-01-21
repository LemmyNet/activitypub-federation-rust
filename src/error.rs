//! Error messages returned by this library

use crate::fetch::webfinger::WebFingerError;
use http_signature_normalization_reqwest::SignError;
use rsa::{
    errors::Error as RsaError,
    pkcs8::{spki::Error as SpkiError, Error as Pkcs8Error},
};
use std::string::FromUtf8Error;
use tokio::task::JoinError;
use url::Url;

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
    /// Failed to serialize outgoing activity
    #[error("Failed to serialize outgoing activity {1}: {0}")]
    SerializeOutgoingActivity(serde_json::Error, String),
    /// Failed to parse an object fetched from url
    #[error("Failed to parse object {1} with content {2}: {0}")]
    ParseFetchedObject(serde_json::Error, Url, String),
    /// Failed to parse an activity received from another instance
    #[error("Failed to parse incoming activity {}: {0}", match .1 {
        Some(t) => format!("with id {t}"),
        None => String::new(),
    })]
    ParseReceivedActivity(serde_json::Error, Option<Url>),
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
    /// Failed to queue activity for sending
    #[error("Failed to queue activity {0} for sending")]
    ActivityQueueError(Url),
    /// Stop activity queue
    #[error(transparent)]
    StopActivityQueue(#[from] JoinError),
    /// Attempted to fetch object which doesn't have valid ActivityPub Content-Type
    #[error(
        "Attempted to fetch object from {0} which doesn't have valid ActivityPub Content-Type"
    )]
    FetchInvalidContentType(Url),
    /// Attempted to fetch object but the response's id field doesn't match
    #[error("Attempted to fetch object from {0} but the response's id field doesn't match")]
    FetchWrongId(Url),
    /// Other generic errors
    #[error("{0}")]
    Other(String),
}

impl From<RsaError> for Error {
    fn from(value: RsaError) -> Self {
        Error::Other(value.to_string())
    }
}

impl From<Pkcs8Error> for Error {
    fn from(value: Pkcs8Error) -> Self {
        Error::Other(value.to_string())
    }
}

impl From<SpkiError> for Error {
    fn from(value: SpkiError) -> Self {
        Error::Other(value.to_string())
    }
}

impl From<url::ParseError> for Error {
    fn from(value: url::ParseError) -> Self {
        Error::UrlParse(value)
    }
}

impl From<WebFingerError> for Error {
    fn from(value: WebFingerError) -> Self {
        Error::WebfingerResolveFailed(value)
    }
}

impl From<FromUtf8Error> for Error {
    fn from(value: FromUtf8Error) -> Self {
        Error::Utf8(value)
    }
}

impl From<SignError> for Error {
    fn from(value: SignError) -> Self {
        Error::SignError(value)
    }
}

impl From<reqwest::Error> for Error {
    fn from(value: reqwest::Error) -> Self {
        Error::Reqwest(value)
    }
}

impl From<reqwest_middleware::Error> for Error {
    fn from(value: reqwest_middleware::Error) -> Self {
        Error::ReqwestMiddleware(value)
    }
}

impl PartialEq for Error {
    fn eq(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}
