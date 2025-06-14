use crate::{config::Data, error::Error, fetch::fetch_object_http, traits::Object};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use std::{
    fmt::{Debug, Display, Formatter},
    marker::PhantomData,
    str::FromStr,
};
use url::Url;

impl<T> FromStr for ObjectId<T>
where
    T: Object + Send + Debug + 'static,
    for<'de2> <T as Object>::Kind: Deserialize<'de2>,
{
    type Err = url::ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ObjectId::parse(s)
    }
}
/// Typed wrapper for Activitypub Object ID which helps with dereferencing and caching.
///
/// It provides convenient methods for fetching the object from remote server or local database.
/// Objects are automatically cached locally, so they don't have to be fetched every time. Much of
/// the crate functionality relies on this wrapper.
///
/// Every time an object is fetched via HTTP, [RequestData.request_counter] is incremented by one.
/// If the value exceeds [FederationSettings.http_fetch_limit], the request is aborted with
/// [Error::RequestLimit]. This prevents denial of service attacks where an attack triggers
/// infinite, recursive fetching of data.
///
/// ```
/// # use activitypub_federation::fetch::object_id::ObjectId;
/// # use activitypub_federation::config::FederationConfig;
/// # use activitypub_federation::error::Error::NotFound;
/// # use activitypub_federation::traits::tests::{DbConnection, DbUser};
/// # tokio::runtime::Runtime::new().unwrap().block_on(async {
/// # let db_connection = DbConnection;
/// let config = FederationConfig::builder()
///     .domain("example.com")
///     .app_data(db_connection)
///     .build().await?;
/// let request_data = config.to_request_data();
/// let object_id = ObjectId::<DbUser>::parse("https://lemmy.ml/u/nutomic")?;
/// // Attempt to fetch object from local database or fall back to remote server
/// let user = object_id.dereference(&request_data).await;
/// assert!(user.is_ok());
/// // Now you can also read the object from local database without network requests
/// let user = object_id.dereference_local(&request_data).await;
/// assert!(user.is_ok());
/// # Ok::<(), anyhow::Error>(())
/// # }).unwrap();
/// ```
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub struct ObjectId<Kind>(Box<Url>, PhantomData<Kind>)
where
    Kind: Object,
    for<'de2> <Kind as Object>::Kind: Deserialize<'de2>;

impl<Kind> ObjectId<Kind>
where
    Kind: Object + Send + Debug + 'static,
    for<'de2> <Kind as Object>::Kind: Deserialize<'de2>,
{
    /// Construct a new objectid instance
    pub fn parse(url: &str) -> Result<Self, url::ParseError> {
        Ok(Self(Box::new(Url::parse(url)?), PhantomData::<Kind>))
    }

    /// Returns a reference to the wrapped URL value
    pub fn inner(&self) -> &Url {
        &self.0
    }

    /// Returns the wrapped URL value
    pub fn into_inner(self) -> Url {
        *self.0
    }

    /// Fetches an activitypub object, either from local database (if possible), or over http.
    pub async fn dereference(
        &self,
        data: &Data<<Kind as Object>::DataType>,
    ) -> Result<Kind, <Kind as Object>::Error>
    where
        <Kind as Object>::Error: From<Error>,
    {
        let db_object = self.dereference_from_db(data).await?;

        // object found in database
        if let Some(object) = db_object {
            if let Some(last_refreshed_at) = object.last_refreshed_at() {
                let is_local = self.is_local(data);
                if !is_local && should_refetch_object(last_refreshed_at) {
                    // object is outdated and should be refetched
                    return self.dereference_from_http(data, Some(object)).await;
                }
            }
            Ok(object)
        }
        // object not found, need to fetch over http
        else {
            self.dereference_from_http(data, None).await
        }
    }

    /// If this is a remote object, fetch it from origin instance unconditionally to get the
    /// latest version, regardless of refresh interval.
    pub async fn dereference_forced(
        &self,
        data: &Data<<Kind as Object>::DataType>,
    ) -> Result<Kind, <Kind as Object>::Error>
    where
        <Kind as Object>::Error: From<Error>,
    {
        if data.config.is_local_url(&self.0) {
            self.dereference_from_db(data)
                .await
                .map(|o| o.ok_or(Error::NotFound.into()))?
        } else {
            // Don't pass in any db object, otherwise it would be returned in case http fetch fails
            self.dereference_from_http(data, None).await
        }
    }

    /// Fetch an object from the local db. Instead of falling back to http, this throws an error if
    /// the object is not found in the database.
    pub async fn dereference_local(
        &self,
        data: &Data<<Kind as Object>::DataType>,
    ) -> Result<Kind, <Kind as Object>::Error>
    where
        <Kind as Object>::Error: From<Error>,
    {
        let object = self.dereference_from_db(data).await?;
        object.ok_or_else(|| Error::NotFound.into())
    }

    /// returning none means the object was not found in local db
    async fn dereference_from_db(
        &self,
        data: &Data<<Kind as Object>::DataType>,
    ) -> Result<Option<Kind>, <Kind as Object>::Error> {
        let id = self.0.clone();
        Object::read_from_id(*id, data).await
    }

    /// Fetch object from origin instance over HTTP, then verify and parse it.
    ///
    /// Uses Box::pin to wrap futures to reduce stack size and avoid stack overflow when
    /// when fetching objects recursively.
    async fn dereference_from_http(
        &self,
        data: &Data<<Kind as Object>::DataType>,
        db_object: Option<Kind>,
    ) -> Result<Kind, <Kind as Object>::Error>
    where
        <Kind as Object>::Error: From<Error>,
    {
        let res = Box::pin(fetch_object_http(&self.0, data)).await;

        if let Err(Error::ObjectDeleted(url)) = res {
            if let Some(db_object) = db_object {
                db_object.delete(data).await?;
            }
            return Err(Error::ObjectDeleted(url).into());
        }

        // If fetch failed, return the existing object from local database
        if let (Err(_), Some(db_object)) = (&res, db_object) {
            return Ok(db_object);
        }
        let res = res?;
        let redirect_url = &res.url;

        // Prevent overwriting local object
        if data.config.is_local_url(redirect_url) {
            return self
                .dereference_from_db(data)
                .await?
                .ok_or(Error::NotFound.into());
        }

        Box::pin(Kind::verify(&res.object, redirect_url, data)).await?;
        Box::pin(Kind::from_json(res.object, data)).await
    }

    /// Returns true if the object's domain matches the one defined in [[FederationConfig.domain]].
    pub fn is_local(&self, data: &Data<<Kind as Object>::DataType>) -> bool {
        data.config.is_local_url(&self.0)
    }
}

/// Need to implement clone manually, to avoid requiring Kind to be Clone
impl<Kind> Clone for ObjectId<Kind>
where
    Kind: Object,
    for<'de2> <Kind as Object>::Kind: Deserialize<'de2>,
{
    fn clone(&self) -> Self {
        ObjectId(self.0.clone(), self.1)
    }
}

static ACTOR_REFETCH_INTERVAL_SECONDS: i64 = 24 * 60 * 60;
static ACTOR_REFETCH_INTERVAL_SECONDS_DEBUG: i64 = 20;

/// Determines when a remote actor should be refetched from its instance. In release builds, this is
/// `ACTOR_REFETCH_INTERVAL_SECONDS` after the last refetch, in debug builds
/// `ACTOR_REFETCH_INTERVAL_SECONDS_DEBUG`.
fn should_refetch_object(last_refreshed: DateTime<Utc>) -> bool {
    let update_interval = if cfg!(debug_assertions) {
        // avoid infinite loop when fetching community outbox
        ChronoDuration::try_seconds(ACTOR_REFETCH_INTERVAL_SECONDS_DEBUG).expect("valid duration")
    } else {
        ChronoDuration::try_seconds(ACTOR_REFETCH_INTERVAL_SECONDS).expect("valid duration")
    };
    let refresh_limit = Utc::now() - update_interval;
    last_refreshed.lt(&refresh_limit)
}

impl<Kind> Display for ObjectId<Kind>
where
    Kind: Object,
    for<'de2> <Kind as Object>::Kind: Deserialize<'de2>,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.as_str())
    }
}

impl<Kind> Debug for ObjectId<Kind>
where
    Kind: Object,
    for<'de2> <Kind as Object>::Kind: Deserialize<'de2>,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.as_str())
    }
}

impl<Kind> From<ObjectId<Kind>> for Url
where
    Kind: Object,
    for<'de2> <Kind as Object>::Kind: Deserialize<'de2>,
{
    fn from(id: ObjectId<Kind>) -> Self {
        *id.0
    }
}

impl<Kind> From<Url> for ObjectId<Kind>
where
    Kind: Object + Send + 'static,
    for<'de2> <Kind as Object>::Kind: Deserialize<'de2>,
{
    fn from(url: Url) -> Self {
        ObjectId(Box::new(url), PhantomData::<Kind>)
    }
}

impl<Kind> PartialEq for ObjectId<Kind>
where
    Kind: Object,
    for<'de2> <Kind as Object>::Kind: Deserialize<'de2>,
{
    fn eq(&self, other: &Self) -> bool {
        self.0.eq(&other.0) && self.1 == other.1
    }
}

/// Internal only
#[cfg(test)]
#[allow(clippy::unwrap_used)]
pub mod tests {
    use super::*;
    use crate::traits::tests::DbUser;

    #[test]
    fn test_deserialize() {
        let id = ObjectId::<DbUser>::parse("http://test.com/").unwrap();

        let string = serde_json::to_string(&id).unwrap();
        assert_eq!("\"http://test.com/\"", string);

        let parsed: ObjectId<DbUser> = serde_json::from_str(&string).unwrap();
        assert_eq!(parsed, id);
    }

    #[test]
    fn test_should_refetch_object() {
        let one_second_ago = Utc::now() - ChronoDuration::try_seconds(1).unwrap();
        assert!(!should_refetch_object(one_second_ago));

        let two_days_ago = Utc::now() - ChronoDuration::try_days(2).unwrap();
        assert!(should_refetch_object(two_days_ago));
    }
}
