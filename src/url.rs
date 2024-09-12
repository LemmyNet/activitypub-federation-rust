//! Wrapper for `url::Url` type.

use serde::{Deserialize, Serialize};
use std::{
    fmt::{Display, Formatter},
    ops::Deref,
    str::FromStr,
};

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
    #[allow(clippy::to_string_in_format_args)]
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.to_string())
    }
}

impl Url {
    /// Returns domain of the url
    pub fn domain(&self) -> &str {
        self.0.domain().expect("has domain")
    }
}

impl TryFrom<url::Url> for Url {
    type Error = url::ParseError;
    fn try_from(value: url::Url) -> Result<Self, Self::Error> {
        if value.domain().is_none() {
            return Err(url::ParseError::EmptyHost);
        }
        Ok(Url(value))
    }
}

#[allow(clippy::from_over_into)]
impl Into<url::Url> for Url {
    fn into(self) -> url::Url {
        self.0
    }
}

impl FromStr for Url {
    type Err = url::ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let url = url::Url::from_str(s)?;
        if url.domain().is_none() {
            return Err(url::ParseError::EmptyHost);
        }
        Ok(Url(url))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod test {
    use super::*;
    use crate::error::Error;
    use std::str::FromStr;

    #[tokio::test]
    async fn test_url() -> Result<(), Error> {
        assert!(Url::from_str("http://example.com").is_ok());
        assert!(Url::try_from(url::Url::from_str("http://example.com")?).is_ok());

        assert!(Url::from_str("http://127.0.0.1").is_err());
        assert!(Url::try_from(url::Url::from_str("http://127.0.0.1")?).is_err());
        Ok(())
    }
}
