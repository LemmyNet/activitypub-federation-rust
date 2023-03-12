use crate::{error::Error, traits::ApubCollection};
use serde::{Deserialize, Serialize};
use std::{
    fmt::{Debug, Display, Formatter},
    marker::PhantomData,
};
use url::Url;

/// TODO: implement and document this, update trait docs
/// TODO: which handlers need to receive owner, and should it be simply an url or what?
/// TODO: current trait impl without read_from_apub_id() method wont work for http handlers
///       -> maybe remove method into_apub() and instead add read_local() (reads from db and
///          directly returns serialized collection)
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub struct CollectionId<Kind>(Box<Url>, PhantomData<Kind>);

impl<Kind> CollectionId<Kind> {
    /// Construct a new CollectionId instance
    pub fn parse<T>(url: T) -> Result<Self, url::ParseError>
    where
        T: TryInto<Url>,
        url::ParseError: From<<T as TryInto<Url>>::Error>,
    {
        Ok(Self(Box::new(url.try_into()?), PhantomData::<Kind>))
    }

    /// TODO
    pub async fn dereference<T>(&self, _: T) -> Result<Kind, Error> {
        todo!()
    }
}

/// Need to implement clone manually, to avoid requiring Kind to be Clone
impl<Kind> Clone for CollectionId<Kind> {
    fn clone(&self) -> Self {
        CollectionId(self.0.clone(), self.1)
    }
}

impl<Kind> Display for CollectionId<Kind>
where
    Kind: ApubCollection,
    for<'de2> <Kind as ApubCollection>::ApubType: serde::Deserialize<'de2>,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.as_str())
    }
}

impl<Kind> Debug for CollectionId<Kind>
where
    Kind: ApubCollection,
    for<'de2> <Kind as ApubCollection>::ApubType: serde::Deserialize<'de2>,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.as_str())
    }
}
impl<Kind> From<CollectionId<Kind>> for Url
where
    Kind: ApubCollection,
    for<'de2> <Kind as ApubCollection>::ApubType: serde::Deserialize<'de2>,
{
    fn from(id: CollectionId<Kind>) -> Self {
        *id.0
    }
}

impl<Kind> From<Url> for CollectionId<Kind>
where
    Kind: ApubCollection + Send + 'static,
    for<'de2> <Kind as ApubCollection>::ApubType: serde::Deserialize<'de2>,
{
    fn from(url: Url) -> Self {
        CollectionId(Box::new(url), PhantomData::<Kind>)
    }
}
