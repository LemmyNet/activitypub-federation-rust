use crate::{config::Data, error::Error};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use either::Either;
use url::Url;

use super::{Actor, Object};

#[async_trait]
impl<T, R, E, D> Object for Either<T, R>
where
    T: Object + Object<Error = E, DataType = D> + Send,
    R: Object + Object<Error = E, DataType = D> + Send,
    <T as Object>::Kind: Send + Sync,
    <R as Object>::Kind: Send + Sync,
    D: Sync + Send + Clone,
    E: From<Error>,
{
    type DataType = D;
    type Kind = Either<T::Kind, R::Kind>;
    type Error = E;

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
        Err(Error::NotFound.into())
    }

    async fn delete(self, data: &Data<Self::DataType>) -> Result<(), Self::Error> {
        match self {
            Either::Left(l) => l.delete(data).await,
            Either::Right(r) => r.delete(data).await,
        }
    }

    async fn into_json(self, data: &Data<Self::DataType>) -> Result<Self::Kind, Self::Error> {
        Ok(match self {
            Either::Left(l) => Either::Left(l.into_json(data).await?),
            Either::Right(r) => Either::Right(r.into_json(data).await?),
        })
    }

    async fn verify(
        json: &Self::Kind,
        expected_domain: &Url,
        data: &Data<Self::DataType>,
    ) -> Result<(), Self::Error> {
        match json {
            Either::Left(l) => T::verify(l, expected_domain, data).await?,
            Either::Right(r) => R::verify(r, expected_domain, data).await?,
        };
        Ok(())
    }

    async fn from_json(json: Self::Kind, data: &Data<Self::DataType>) -> Result<Self, Self::Error> {
        Ok(match json {
            Either::Left(l) => Either::Left(T::from_json(l, data).await?),
            Either::Right(r) => Either::Right(R::from_json(r, data).await?),
        })
    }
}

#[async_trait]
impl<T, R, E, D> Actor for Either<T, R>
where
    T: Actor + Object + Object<Error = E, DataType = D> + Send + 'static,
    R: Actor + Object + Object<Error = E, DataType = D> + Send + 'static,
    <T as Object>::Kind: Send + Sync,
    <R as Object>::Kind: Send + Sync,
    D: Sync + Send + Clone,
    E: From<Error>,
{
    fn id(&self) -> Url {
        match self {
            Either::Left(l) => l.id(),
            Either::Right(r) => r.id(),
        }
    }

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
