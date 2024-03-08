use crate::{
    config::Data,
    error::Error,
    fetch::{fetch_object_http_with_accept, object_id::ObjectId},
    traits::{Actor, Object},
    FEDERATION_CONTENT_TYPE,
};
use http::HeaderValue;
use itertools::Itertools;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt::Display};
use tracing::debug;
use url::Url;

/// Errors relative to webfinger handling
#[derive(thiserror::Error, Debug)]
pub enum WebFingerError {
    /// The webfinger identifier is invalid
    #[error("The webfinger identifier is invalid")]
    WrongFormat,
    /// The webfinger identifier doesn't match the expected instance domain name
    #[error("The webfinger identifier doesn't match the expected instance domain name")]
    WrongDomain,
    /// The wefinger object did not contain any link to an activitypub item
    #[error("The webfinger object did not contain any link to an activitypub item")]
    NoValidLink,
}

impl WebFingerError {
    fn into_crate_error(self) -> Error {
        self.into()
    }
}

/// The content-type for webfinger responses.
pub static WEBFINGER_CONTENT_TYPE: HeaderValue = HeaderValue::from_static("application/jrd+json");

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
    <Kind as Object>::Error: From<crate::error::Error> + Send + Sync + Display,
{
    let (_, domain) = identifier
        .splitn(2, '@')
        .collect_tuple()
        .ok_or(WebFingerError::WrongFormat.into_crate_error())?;
    let protocol = if data.config.debug { "http" } else { "https" };
    let fetch_url =
        format!("{protocol}://{domain}/.well-known/webfinger?resource=acct:{identifier}");
    debug!("Fetching webfinger url: {}", &fetch_url);

    let res: Webfinger = fetch_object_http_with_accept(
        &Url::parse(&fetch_url).map_err(Error::UrlParse)?,
        data,
        &WEBFINGER_CONTENT_TYPE,
    )
    .await?
    .object;

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
        match object {
            Ok(obj) => return Ok(obj),
            Err(error) => debug!(%error, "Failed to dereference link"),
        }
    }
    Err(WebFingerError::NoValidLink.into_crate_error().into())
}

/// Extracts username from a webfinger resource parameter.
///
/// Use this method for your HTTP handler at `.well-known/webfinger` to handle incoming webfinger
/// request. For a parameter of the form `acct:gargron@mastodon.social` it returns `gargron`.
///
/// Returns an error if query doesn't match local domain.
///
///```
/// # use activitypub_federation::config::FederationConfig;
/// # use activitypub_federation::traits::tests::DbConnection;
/// # use activitypub_federation::fetch::webfinger::extract_webfinger_name;
/// # tokio::runtime::Runtime::new().unwrap().block_on(async {
/// # let db_connection = DbConnection;
/// let config = FederationConfig::builder()
///     .domain("example.com")
///     .app_data(db_connection)
///     .build()
///     .await
///     .unwrap();
/// let data = config.to_request_data();
/// let res = extract_webfinger_name("acct:test_user@example.com", &data).unwrap();
/// assert_eq!(res, "test_user");
/// # Ok::<(), anyhow::Error>(())
/// }).unwrap();
///```
pub fn extract_webfinger_name<'i, T>(query: &'i str, data: &Data<T>) -> Result<&'i str, Error>
where
    T: Clone,
{
    static WEBFINGER_REGEX: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"^acct:([\p{L}0-9_\.\-]+)@(.*)$").expect("compile regex"));
    // Regex to extract usernames from webfinger query. Supports different alphabets using `\p{L}`.
    // TODO: This should use a URL parser
    let captures = WEBFINGER_REGEX
        .captures(query)
        .ok_or(WebFingerError::WrongFormat)?;

    let account_name = captures.get(1).ok_or(WebFingerError::WrongFormat)?;

    if captures.get(2).map(|m| m.as_str()) != Some(data.domain()) {
        return Err(WebFingerError::WrongDomain.into());
    }
    Ok(account_name.as_str())
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
/// build_webfinger_response_with_type(subject, vec![
///     (user, Some("Person")),
///     (group, Some("Group"))]);
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn build_webfinger_response_with_type(
    subject: String,
    urls: Vec<(Url, Option<&str>)>,
) -> Webfinger {
    Webfinger {
        subject,
        links: urls.iter().fold(vec![], |mut acc, (url, kind)| {
            let properties: HashMap<Url, String> = kind
                .map(|kind| {
                    HashMap::from([(
                        "https://www.w3.org/ns/activitystreams#type"
                            .parse()
                            .expect("parse url"),
                        kind.to_string(),
                    )])
                })
                .unwrap_or_default();
            let mut links = vec![
                WebfingerLink {
                    rel: Some("http://webfinger.net/rel/profile-page".to_string()),
                    kind: Some("text/html".to_string()),
                    href: Some(url.clone()),
                    ..Default::default()
                },
                WebfingerLink {
                    rel: Some("self".to_string()),
                    kind: Some(FEDERATION_CONTENT_TYPE.to_string()),
                    href: Some(url.clone()),
                    properties,
                    ..Default::default()
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
    /// Used for remote follow external interaction url
    pub template: Option<String>,
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

    #[tokio::test]
    async fn test_webfinger() -> Result<(), Error> {
        let config = FederationConfig::builder()
            .domain("example.com")
            .app_data(DbConnection)
            .build()
            .await
            .unwrap();
        let data = config.to_request_data();

        webfinger_resolve_actor::<DbConnection, DbUser>("LemmyDev@mastodon.social", &data).await?;
        // poa.st is as of 2023-07-14 the largest Pleroma instance
        webfinger_resolve_actor::<DbConnection, DbUser>("graf@poa.st", &data).await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_webfinger_extract_name() -> Result<(), Error> {
        use crate::traits::tests::DbConnection;
        let data = Data {
            config: FederationConfig::builder()
                .domain("example.com")
                .app_data(DbConnection)
                .build()
                .await
                .unwrap(),
            request_counter: Default::default(),
        };
        assert_eq!(
            Ok("test123"),
            extract_webfinger_name("acct:test123@example.com", &data)
        );
        assert_eq!(
            Ok("Владимир"),
            extract_webfinger_name("acct:Владимир@example.com", &data)
        );
        assert_eq!(
            Ok("example.com"),
            extract_webfinger_name("acct:example.com@example.com", &data)
        );
        assert_eq!(
            Ok("da-sh"),
            extract_webfinger_name("acct:da-sh@example.com", &data)
        );
        assert_eq!(
            Ok("تجريب"),
            extract_webfinger_name("acct:تجريب@example.com", &data)
        );
        Ok(())
    }
}
