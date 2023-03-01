use crate::{
    config::RequestData,
    core::object_id::ObjectId,
    error::{Error, Error::WebfingerResolveFailed},
    traits::{Actor, ApubObject},
    utils::fetch_object_http,
    APUB_JSON_CONTENT_TYPE,
};
use anyhow::anyhow;
use itertools::Itertools;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::debug;
use url::Url;

/// Turns a person id like `@name@example.com` into an apub ID, like `https://example.com/user/name`,
/// using webfinger.
pub async fn webfinger_resolve_actor<T: Clone, Kind>(
    identifier: &str,
    data: &RequestData<T>,
) -> Result<Kind, <Kind as ApubObject>::Error>
where
    Kind: ApubObject + Actor + Send + 'static + ApubObject<DataType = T>,
    for<'de2> <Kind as ApubObject>::ApubType: serde::Deserialize<'de2>,
    <Kind as ApubObject>::Error:
        From<crate::error::Error> + From<anyhow::Error> + From<url::ParseError> + Send + Sync,
{
    let (_, domain) = identifier
        .splitn(2, '@')
        .collect_tuple()
        .ok_or_else(|| WebfingerResolveFailed)?;
    let protocol = if data.config.debug { "http" } else { "https" };
    let fetch_url =
        format!("{protocol}://{domain}/.well-known/webfinger?resource=acct:{identifier}");
    debug!("Fetching webfinger url: {}", &fetch_url);

    let res: Webfinger = fetch_object_http(&Url::parse(&fetch_url)?, data).await?;

    let links: Vec<Url> = res
        .links
        .iter()
        .filter(|link| {
            if let Some(type_) = &link.kind {
                type_.starts_with("application/")
            } else {
                false
            }
        })
        .filter_map(|l| l.href.clone())
        .collect();
    for l in links {
        let object = ObjectId::<Kind>::from(l).dereference(data).await;
        if object.is_ok() {
            return object;
        }
    }
    Err(WebfingerResolveFailed.into())
}

/// Extracts username from a webfinger resource parameter.
///
/// For a parameter of the form `acct:gargron@mastodon.social` it returns `gargron`.
///
/// Returns an error if query doesn't match local domain.
pub fn extract_webfinger_name<T>(query: &str, data: &RequestData<T>) -> Result<String, Error>
where
    T: Clone,
{
    // TODO: would be nice if we could implement this without regex and remove the dependency
    let regex = Regex::new(&format!("^acct:([a-zA-Z0-9_]{{3,}})@{}$", data.domain()))
        .map_err(Error::other)?;
    Ok(regex
        .captures(query)
        .and_then(|c| c.get(1))
        .ok_or_else(|| Error::other(anyhow!("Webfinger regex failed to match")))?
        .as_str()
        .to_string())
}

/// Builds a basic webfinger response under the assumption that `html` and `activity+json`
/// links are identical.
pub fn build_webfinger_response(resource: String, url: Url) -> Webfinger {
    Webfinger {
        subject: resource,
        links: vec![
            WebfingerLink {
                rel: Some("http://webfinger.net/rel/profile-page".to_string()),
                kind: Some("text/html".to_string()),
                href: Some(url.clone()),
                properties: Default::default(),
            },
            WebfingerLink {
                rel: Some("self".to_string()),
                kind: Some(APUB_JSON_CONTENT_TYPE.to_string()),
                href: Some(url),
                properties: Default::default(),
            },
        ],
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Webfinger {
    pub subject: String,
    pub links: Vec<WebfingerLink>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct WebfingerLink {
    pub rel: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub href: Option<Url>,
    #[serde(default)]
    pub properties: HashMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::FederationConfig,
        traits::tests::{DbConnection, DbUser},
    };

    #[actix_rt::test]
    async fn test_webfinger() {
        let config = FederationConfig::builder()
            .domain("example.com")
            .app_data(DbConnection)
            .build()
            .unwrap();
        let data = config.to_request_data();
        let res =
            webfinger_resolve_actor::<DbConnection, DbUser>("LemmyDev@mastodon.social", &data)
                .await;
        assert!(res.is_ok());
    }
}
