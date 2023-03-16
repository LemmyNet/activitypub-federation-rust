use crate::{config::Data, error::Error, fetch::fetch_object_http, traits::Collection};
use serde::{Deserialize, Serialize};
use std::{
    fmt::{Debug, Display, Formatter},
    marker::PhantomData,
};
use url::Url;

/// Typed wrapper for Activitypub Collection ID which helps with dereferencing.
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub struct CollectionId<Kind>(Box<Url>, PhantomData<Kind>)
where
    Kind: Collection,
    for<'de2> <Kind as Collection>::Kind: Deserialize<'de2>;

impl<Kind> CollectionId<Kind>
where
    Kind: Collection,
    for<'de2> <Kind as Collection>::Kind: Deserialize<'de2>,
{
    /// Construct a new CollectionId instance
    pub fn parse<T>(url: T) -> Result<Self, url::ParseError>
    where
        T: TryInto<Url>,
        url::ParseError: From<<T as TryInto<Url>>::Error>,
    {
        Ok(Self(Box::new(url.try_into()?), PhantomData::<Kind>))
    }

    /// Fetches collection over HTTP
    ///
    /// Unlike [ObjectId::fetch](crate::fetch::object_id::ObjectId::fetch) this method doesn't do
    /// any caching.
    pub async fn dereference(
        &self,
        owner: &<Kind as Collection>::Owner,
        data: &Data<<Kind as Collection>::DataType>,
    ) -> Result<Kind, <Kind as Collection>::Error>
    where
        <Kind as Collection>::Error: From<Error>,
    {
        let json = fetch_object_http(&self.0, data).await?;
        Kind::verify(&json, &self.0, data).await?;
        Kind::from_json(json, owner, data).await
    }
}

/// Need to implement clone manually, to avoid requiring Kind to be Clone
impl<Kind> Clone for CollectionId<Kind>
where
    Kind: Collection,
    for<'de2> <Kind as Collection>::Kind: serde::Deserialize<'de2>,
{
    fn clone(&self) -> Self {
        CollectionId(self.0.clone(), self.1)
    }
}

impl<Kind> Display for CollectionId<Kind>
where
    Kind: Collection,
    for<'de2> <Kind as Collection>::Kind: serde::Deserialize<'de2>,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.as_str())
    }
}

impl<Kind> Debug for CollectionId<Kind>
where
    Kind: Collection,
    for<'de2> <Kind as Collection>::Kind: serde::Deserialize<'de2>,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.as_str())
    }
}
impl<Kind> From<CollectionId<Kind>> for Url
where
    Kind: Collection,
    for<'de2> <Kind as Collection>::Kind: serde::Deserialize<'de2>,
{
    fn from(id: CollectionId<Kind>) -> Self {
        *id.0
    }
}

impl<Kind> From<Url> for CollectionId<Kind>
where
    Kind: Collection + Send + 'static,
    for<'de2> <Kind as Collection>::Kind: serde::Deserialize<'de2>,
{
    fn from(url: Url) -> Self {
        CollectionId(Box::new(url), PhantomData::<Kind>)
    }
}
