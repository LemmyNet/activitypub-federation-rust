use crate::{
    config::Data,
    error::{Error, Error::WebfingerResolveFailed},
    fetch::{fetch_object_http, object_id::ObjectId},
    traits::{Actor, Object},
    FEDERATION_CONTENT_TYPE,
};
use anyhow::anyhow;
use itertools::Itertools;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::debug;
use url::Url;

/// Takes an identifier of the form `name@example.com`, and returns an object of `Kind`.
///
/// For this the identifier is first resolved via webfinger protocol to an Activitypub ID. This ID
/// is then fetched using [ObjectId::dereference], and the result returned.
pub async fn webfinger_resolve_actor<T: Clone, Kind>(
    identifier: &str,
    data: &Data<T>,
) -> Result<Kind, <Kind as Object>::Error>
where
    Kind: Object + Actor + Send + 'static + Object<DataType = T>,
    for<'de2> <Kind as Object>::Kind: serde::Deserialize<'de2>,
    <Kind as Object>::Error:
        From<crate::error::Error> + From<anyhow::Error> + From<url::ParseError> + Send + Sync,
{
    let (_, domain) = identifier
        .splitn(2, '@')
        .collect_tuple()
        .ok_or(WebfingerResolveFailed)?;
    let protocol = if data.config.debug { "http" } else { "https" };
    let fetch_url =
        format!("{protocol}://{domain}/.well-known/webfinger?resource=acct:{identifier}");
    debug!("Fetching webfinger url: {}", &fetch_url);

    let res: Webfinger = fetch_object_http(&Url::parse(&fetch_url)?, data).await?;

    debug_assert_eq!(res.subject, format!("acct:{identifier}"));
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
/// Use this method for your HTTP handler at `.well-known/webfinger` to handle incoming webfinger
/// request. For a parameter of the form `acct:gargron@mastodon.social` it returns `gargron`.
///
/// Returns an error if query doesn't match local domain.
pub fn extract_webfinger_name<T>(query: &str, data: &Data<T>) -> Result<String, Error>
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

/// Builds a basic webfinger response for the actor.
///
/// It assumes that the given URL is valid both to the view the actor in a browser as HTML, and
/// for fetching it over Activitypub with `activity+json`. This setup is commonly used for ease
/// of discovery.
///
/// ```
/// # use url::Url;
/// # use activitypub_federation::fetch::webfinger::build_webfinger_response;
/// let subject = "acct:nutomic@lemmy.ml".to_string();
/// let url = Url::parse("https://lemmy.ml/u/nutomic")?;
/// build_webfinger_response(subject, url);
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn build_webfinger_response(subject: String, url: Url) -> Webfinger {
    build_webfinger_response_with_type(subject, vec![(url, None)])
}

/// Builds a webfinger response similar to `build_webfinger_response`. Use this when you want to
/// return multiple actors who share the same namespace and to specify the type of the actor.
///
/// `urls` takes a vector of tuples. The first item of the tuple is the URL while the second
/// item is the type, such as `"Person"` or `"Group"`. If `None` is passed for the type, the field
/// will be empty.
///
/// ```
/// # use url::Url;
/// # use activitypub_federation::fetch::webfinger::build_webfinger_response_with_type;
/// let subject = "acct:nutomic@lemmy.ml".to_string();
/// let user = Url::parse("https://lemmy.ml/u/nutomic")?;
/// let group = Url::parse("https://lemmy.ml/c/asklemmy")?;
/// let other = Url::parse("https://lemmy.ml/c/memes")?;
/// build_webfinger_response_with_type(subject, vec![
///     (user, Some("Person")),
///     (group, Some("Group")),
///     (other, None)]);
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn build_webfinger_response_with_type(
    subject: String,
    urls: Vec<(Url, Option<&str>)>,
) -> Webfinger {
    Webfinger {
        subject,
        links: urls.iter().fold(vec![], |mut acc, (url, kind)| {
            let mut links = vec![
                WebfingerLink {
                    rel: Some("http://webfinger.net/rel/profile-page".to_string()),
                    kind: Some("text/html".to_string()),
                    href: Some(url.clone()),
                    properties: Default::default(),
                },
                WebfingerLink {
                    rel: Some("self".to_string()),
                    kind: Some(FEDERATION_CONTENT_TYPE.to_string()),
                    href: Some(url.clone()),
                    properties: kind
                        .map(|kind| {
                            HashMap::from([(
                                "https://www.w3.org/ns/activitystreams#type"
                                    .parse::<Url>()
                                    .expect("parse url"),
                                kind.to_string(),
                            )])
                        })
                        .unwrap_or_default(),
                },
            ];
            acc.append(&mut links);
            acc
        }),
        aliases: vec![],
        properties: Default::default(),
    }
}

/// A webfinger response with information about a `Person` or other type of actor.
#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Webfinger {
    /// The actor which is described here, for example `acct:LemmyDev@mastodon.social`
    pub subject: String,
    /// Links where further data about `subject` can be retrieved
    pub links: Vec<WebfingerLink>,
    /// Other Urls which identify the same actor as the `subject`
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<Url>,
    /// Additional data about the subject
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub properties: HashMap<Url, String>,
}

/// A single link included as part of a [Webfinger] response.
#[derive(Serialize, Deserialize, Debug, Default)]
pub struct WebfingerLink {
    /// Relationship of the link, such as `self` or `http://webfinger.net/rel/profile-page`
    pub rel: Option<String>,
    /// Media type of the target resource
    #[serde(rename = "type")]
    pub kind: Option<String>,
    /// Url pointing to the target resource
    pub href: Option<Url>,
    /// Additional data about the link
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub properties: HashMap<Url, String>,
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
