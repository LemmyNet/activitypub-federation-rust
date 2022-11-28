use crate::{traits::ApubObject, utils::fetch_object_http, Error, LocalInstance};
use anyhow::anyhow;
use chrono::{Duration as ChronoDuration, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{
    fmt::{Debug, Display, Formatter},
    marker::PhantomData,
};
use url::Url;

/// We store Url on the heap because it is quite large (88 bytes).
#[derive(Serialize, Deserialize, Debug)]
#[serde(transparent)]
pub struct ObjectId<Kind>(Box<Url>, PhantomData<Kind>)
where
    Kind: ApubObject + Send + 'static,
    for<'de2> <Kind as ApubObject>::ApubType: serde::Deserialize<'de2>;

impl<Kind> ObjectId<Kind>
where
    Kind: ApubObject + Send + 'static,
    for<'de2> <Kind as ApubObject>::ApubType: serde::Deserialize<'de2>,
{
    pub fn new<T>(url: T) -> Self
    where
        T: Into<Url>,
    {
        ObjectId(Box::new(url.into()), PhantomData::<Kind>)
    }

    pub fn inner(&self) -> &Url {
        &self.0
    }

    pub fn into_inner(self) -> Url {
        *self.0
    }

    /// Fetches an activitypub object, either from local database (if possible), or over http.
    pub async fn dereference(
        &self,
        data: &<Kind as ApubObject>::DataType,
        instance: &LocalInstance,
        request_counter: &mut i32,
    ) -> Result<Kind, <Kind as ApubObject>::Error>
    where
        <Kind as ApubObject>::Error: From<Error> + From<anyhow::Error>,
    {
        let db_object = self.dereference_from_db(data).await?;

        // if its a local object, only fetch it from the database and not over http
        if instance.is_local_url(&self.0) {
            return match db_object {
                None => Err(Error::NotFound.into()),
                Some(o) => Ok(o),
            };
        }

        // object found in database
        if let Some(object) = db_object {
            // object is old and should be refetched
            if let Some(last_refreshed_at) = object.last_refreshed_at() {
                if should_refetch_object(last_refreshed_at) {
                    return self
                        .dereference_from_http(data, instance, request_counter, Some(object))
                        .await;
                }
            }
            Ok(object)
        }
        // object not found, need to fetch over http
        else {
            self.dereference_from_http(data, instance, request_counter, None)
                .await
        }
    }

    /// Fetch an object from the local db. Instead of falling back to http, this throws an error if
    /// the object is not found in the database.
    pub async fn dereference_local(
        &self,
        data: &<Kind as ApubObject>::DataType,
    ) -> Result<Kind, <Kind as ApubObject>::Error>
    where
        <Kind as ApubObject>::Error: From<Error>,
    {
        let object = self.dereference_from_db(data).await?;
        object.ok_or_else(|| Error::NotFound.into())
    }

    /// returning none means the object was not found in local db
    async fn dereference_from_db(
        &self,
        data: &<Kind as ApubObject>::DataType,
    ) -> Result<Option<Kind>, <Kind as ApubObject>::Error> {
        let id = self.0.clone();
        ApubObject::read_from_apub_id(*id, data).await
    }

    async fn dereference_from_http(
        &self,
        data: &<Kind as ApubObject>::DataType,
        instance: &LocalInstance,
        request_counter: &mut i32,
        db_object: Option<Kind>,
    ) -> Result<Kind, <Kind as ApubObject>::Error>
    where
        <Kind as ApubObject>::Error: From<Error> + From<anyhow::Error>,
    {
        let res = fetch_object_http(&self.0, instance, request_counter).await;

        if let Err(Error::ObjectDeleted) = &res {
            if let Some(db_object) = db_object {
                db_object.delete(data).await?;
            }
            return Err(anyhow!("Fetched remote object {} which was deleted", self).into());
        }

        let res2 = res?;

        Kind::verify(&res2, self.inner(), data, request_counter).await?;
        Kind::from_apub(res2, data, request_counter).await
    }
}

/// Need to implement clone manually, to avoid requiring Kind to be Clone
impl<Kind> Clone for ObjectId<Kind>
where
    Kind: ApubObject + Send + 'static,
    for<'de2> <Kind as ApubObject>::ApubType: serde::Deserialize<'de2>,
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
///
/// TODO it won't pick up new avatars, summaries etc until a day after.
/// Actors need an "update" activity pushed to other servers to fix this.
fn should_refetch_object(last_refreshed: NaiveDateTime) -> bool {
    let update_interval = if cfg!(debug_assertions) {
        // avoid infinite loop when fetching community outbox
        ChronoDuration::seconds(ACTOR_REFETCH_INTERVAL_SECONDS_DEBUG)
    } else {
        ChronoDuration::seconds(ACTOR_REFETCH_INTERVAL_SECONDS)
    };
    let refresh_limit = Utc::now().naive_utc() - update_interval;
    last_refreshed.lt(&refresh_limit)
}

impl<Kind> Display for ObjectId<Kind>
where
    Kind: ApubObject + Send + 'static,
    for<'de2> <Kind as ApubObject>::ApubType: serde::Deserialize<'de2>,
{
    #[allow(clippy::recursive_format_impl)]
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        // Use to_string here because Url.display is not useful for us
        write!(f, "{}", self.0)
    }
}

impl<Kind> From<ObjectId<Kind>> for Url
where
    Kind: ApubObject + Send + 'static,
    for<'de2> <Kind as ApubObject>::ApubType: serde::Deserialize<'de2>,
{
    fn from(id: ObjectId<Kind>) -> Self {
        *id.0
    }
}

impl<Kind> PartialEq for ObjectId<Kind>
where
    Kind: ApubObject + Send + 'static,
    for<'de2> <Kind as ApubObject>::ApubType: serde::Deserialize<'de2>,
{
    fn eq(&self, other: &Self) -> bool {
        self.0.eq(&other.0) && self.1 == other.1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::object_id::should_refetch_object;
    use anyhow::Error;

    #[derive(Debug)]
    struct TestObject {}

    #[async_trait::async_trait]
    impl ApubObject for TestObject {
        type DataType = TestObject;
        type ApubType = ();
        type DbType = ();
        type Error = Error;

        async fn read_from_apub_id(
            _object_id: Url,
            _data: &Self::DataType,
        ) -> Result<Option<Self>, Self::Error>
        where
            Self: Sized,
        {
            todo!()
        }

        async fn into_apub(self, _data: &Self::DataType) -> Result<Self::ApubType, Self::Error> {
            todo!()
        }

        async fn verify(
            _apub: &Self::ApubType,
            _expected_domain: &Url,
            _data: &Self::DataType,
            _request_counter: &mut i32,
        ) -> Result<(), Self::Error> {
            todo!()
        }

        async fn from_apub(
            _apub: Self::ApubType,
            _data: &Self::DataType,
            _request_counter: &mut i32,
        ) -> Result<Self, Self::Error>
        where
            Self: Sized,
        {
            todo!()
        }
    }

    #[test]
    fn test_deserialize() {
        let url = Url::parse("http://test.com/").unwrap();
        let id = ObjectId::<TestObject>::new(url);

        let string = serde_json::to_string(&id).unwrap();
        assert_eq!("\"http://test.com/\"", string);

        let parsed: ObjectId<TestObject> = serde_json::from_str(&string).unwrap();
        assert_eq!(parsed, id);
    }

    #[test]
    fn test_should_refetch_object() {
        let one_second_ago = Utc::now().naive_utc() - ChronoDuration::seconds(1);
        assert!(!should_refetch_object(one_second_ago));

        let two_days_ago = Utc::now().naive_utc() - ChronoDuration::days(2);
        assert!(should_refetch_object(two_days_ago));
    }
}
