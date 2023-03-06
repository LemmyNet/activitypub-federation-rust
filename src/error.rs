//! Error messages returned by this library

use displaydoc::Display;

/// Error messages returned by this library
#[derive(thiserror::Error, Debug, Display)]
pub enum Error {
    /// Object was not found in local database
    NotFound,
    /// Request limit was reached during fetch
    RequestLimit,
    /// Response body limit was reached during fetch
    ResponseBodyLimit,
    /// Object to be fetched was deleted
    ObjectDeleted,
    /// {0}
    UrlVerificationError(&'static str),
    /// Incoming activity has invalid digest for body
    ActivityBodyDigestInvalid,
    /// Incoming activity has invalid signature
    ActivitySignatureInvalid,
    /// Failed to resolve actor via webfinger
    WebfingerResolveFailed,
    /// Other errors which are not explicitly handled
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl Error {
    pub(crate) fn other<T>(error: T) -> Self
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
