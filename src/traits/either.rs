use super::{Actor, Object};
use crate::{config::Data, error::Error};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use either::Either;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use url::Url;

#[doc(hidden)]
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
pub enum UntaggedEither<L, R> {
    Left(L),
    Right(R),
}

#[async_trait]
impl<T, R, E, D> Object for Either<T, R>
where
    T: Object + Object<Error = E, DataType = D> + Send + Sync,
    R: Object + Object<Error = E, DataType = D> + Send + Sync,
    <T as Object>::Kind: Send + Sync,
    <R as Object>::Kind: Send + Sync,
    D: Sync + Send + Clone,
    E: From<Error> + Debug,
{
    type DataType = D;
    type Kind = UntaggedEither<T::Kind, R::Kind>;
    type Error = E;

    /// `id` field of the object
    fn id(&self) -> &Url {
        match self {
            Either::Left(l) => l.id(),
            Either::Right(r) => r.id(),
        }
    }

    fn last_refreshed_at(&self) -> Option<DateTime<Utc>> {
        match self {
            Either::Left(l) => l.last_refreshed_at(),
            Either::Right(r) => r.last_refreshed_at(),
        }
    }

    async fn read_from_id(
        object_id: Url,
        data: &Data<Self::DataType>,
    ) -> Result<Option<Self>, Self::Error> {
        let l = T::read_from_id(object_id.clone(), data).await?;
        if let Some(l) = l {
            return Ok(Some(Either::Left(l)));
        }
        let r = R::read_from_id(object_id.clone(), data).await?;
        if let Some(r) = r {
            return Ok(Some(Either::Right(r)));
        }
        Ok(None)
    }

    async fn delete(&self, data: &Data<Self::DataType>) -> Result<(), Self::Error> {
        match self {
            Either::Left(l) => l.delete(data).await,
            Either::Right(r) => r.delete(data).await,
        }
    }

    fn is_deleted(&self) -> bool {
        match self {
            Either::Left(l) => l.is_deleted(),
            Either::Right(r) => r.is_deleted(),
        }
    }

    async fn into_json(self, data: &Data<Self::DataType>) -> Result<Self::Kind, Self::Error> {
        Ok(match self {
            Either::Left(l) => UntaggedEither::Left(l.into_json(data).await?),
            Either::Right(r) => UntaggedEither::Right(r.into_json(data).await?),
        })
    }

    async fn verify(
        json: &Self::Kind,
        expected_domain: &Url,
        data: &Data<Self::DataType>,
    ) -> Result<(), Self::Error> {
        match json {
            UntaggedEither::Left(l) => T::verify(l, expected_domain, data).await?,
            UntaggedEither::Right(r) => R::verify(r, expected_domain, data).await?,
        };
        Ok(())
    }

    async fn from_json(json: Self::Kind, data: &Data<Self::DataType>) -> Result<Self, Self::Error> {
        Ok(match json {
            UntaggedEither::Left(l) => Either::Left(T::from_json(l, data).await?),
            UntaggedEither::Right(r) => Either::Right(R::from_json(r, data).await?),
        })
    }
}

#[async_trait]
impl<T, R, E, D> Actor for Either<T, R>
where
    T: Actor + Object + Object<Error = E, DataType = D> + Send + Sync + 'static,
    R: Actor + Object + Object<Error = E, DataType = D> + Send + Sync + 'static,
    <T as Object>::Kind: Send + Sync,
    <R as Object>::Kind: Send + Sync,
    D: Sync + Send + Clone,
    E: From<Error> + Debug,
{
    fn public_key_pem(&self) -> &str {
        match self {
            Either::Left(l) => l.public_key_pem(),
            Either::Right(r) => r.public_key_pem(),
        }
    }

    fn private_key_pem(&self) -> Option<String> {
        match self {
            Either::Left(l) => l.private_key_pem(),
            Either::Right(r) => r.private_key_pem(),
        }
    }

    fn inbox(&self) -> Url {
        match self {
            Either::Left(l) => l.inbox(),
            Either::Right(r) => r.inbox(),
        }
    }
}
