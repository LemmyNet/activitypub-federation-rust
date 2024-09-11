//! Wrapper for `url::Url` type.

use std::{
    fmt::{Display, Formatter},
    ops::Deref,
    str::FromStr,
};

use serde::{Deserialize, Serialize};

/// Wrapper for `url::Url` type. Has `domain` as mandatory field, and prints plain
/// string for debugging.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
pub struct Url(url::Url);

impl Deref for Url {
    type Target = url::Url;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Display for Url {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.to_string())
    }
}

impl Url {
    /// Returns domain of the url
    pub fn domain(&self) -> &str {
        // TODO: must have error handling, or ensure at creation that it has domain
        self.0.domain().expect("has domain")
    }
}

impl From<url::Url> for Url {
    fn from(value: url::Url) -> Self {
        Url(value)
    }
}

impl FromStr for Url {
    type Err = url::ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(url::Url::from_str(s).map(Url).unwrap())
    }
}
