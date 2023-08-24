//! Error messages returned by this library

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
    #[error("Object to be fetched was deleted")]
    ObjectDeleted,
    /// url verification error
    #[error("URL failed verification: {0}")]
    UrlVerificationError(anyhow::Error),
    /// Incoming activity has invalid digest for body
    #[error("Incoming activity has invalid digest for body")]
    ActivityBodyDigestInvalid,
    /// Incoming activity has invalid signature
    #[error("Incoming activity has invalid signature")]
    ActivitySignatureInvalid,
    /// Failed to resolve actor via webfinger
    #[error("Failed to resolve actor via webfinger")]
    WebfingerResolveFailed,
    /// other error
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
