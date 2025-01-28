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
    pub fn parse(url: &str) -> Result<Self, url::ParseError> {
        Ok(Self(Box::new(Url::parse(url)?), PhantomData::<Kind>))
    }

    /// Fetches collection over HTTP
    ///
    /// Unlike [ObjectId::dereference](crate::fetch::object_id::ObjectId::dereference) this method doesn't do
    /// any caching.
    pub async fn dereference(
        &self,
        owner: &<Kind as Collection>::Owner,
        data: &Data<<Kind as Collection>::DataType>,
    ) -> Result<Kind, <Kind as Collection>::Error>
    where
        <Kind as Collection>::Error: From<Error>,
    {
        let res = fetch_object_http(&self.0, data).await?;
        let redirect_url = &res.url;
        Kind::verify(&res.object, redirect_url, data).await?;
        Kind::from_json(res.object, owner, data).await
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

impl<Kind> PartialEq for CollectionId<Kind>
where
    Kind: Collection,
    for<'de2> <Kind as Collection>::Kind: serde::Deserialize<'de2>,
{
    fn eq(&self, other: &Self) -> bool {
        self.0.eq(&other.0) && self.1 == other.1
    }
}

#[cfg(feature = "diesel")]
const _: () = {
    use diesel::{
        backend::Backend,
        deserialize::{FromSql, FromStaticSqlRow},
        expression::AsExpression,
        internal::derives::as_expression::Bound,
        pg::Pg,
        query_builder::QueryId,
        serialize,
        serialize::{Output, ToSql},
        sql_types::{HasSqlType, SingleValue, Text},
        Expression,
        Queryable,
    };

    // TODO: this impl only works for Postgres db because of to_string() call which requires reborrow
    impl<Kind, ST> ToSql<ST, Pg> for CollectionId<Kind>
    where
        Kind: Collection,
        for<'de2> <Kind as Collection>::Kind: Deserialize<'de2>,
        String: ToSql<ST, Pg>,
    {
        fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
            let v = self.0.to_string();
            <String as ToSql<Text, Pg>>::to_sql(&v, &mut out.reborrow())
        }
    }
    impl<'expr, Kind, ST> AsExpression<ST> for &'expr CollectionId<Kind>
    where
        Kind: Collection,
        for<'de2> <Kind as Collection>::Kind: Deserialize<'de2>,
        Bound<ST, String>: Expression<SqlType = ST>,
        ST: SingleValue,
    {
        type Expression = Bound<ST, &'expr str>;
        fn as_expression(self) -> Self::Expression {
            Bound::new(self.0.as_str())
        }
    }
    impl<Kind, ST> AsExpression<ST> for CollectionId<Kind>
    where
        Kind: Collection,
        for<'de2> <Kind as Collection>::Kind: Deserialize<'de2>,
        Bound<ST, String>: Expression<SqlType = ST>,
        ST: SingleValue,
    {
        type Expression = Bound<ST, String>;
        fn as_expression(self) -> Self::Expression {
            Bound::new(self.0.to_string())
        }
    }
    impl<Kind, ST, DB> FromSql<ST, DB> for CollectionId<Kind>
    where
        Kind: Collection + Send + 'static,
        for<'de2> <Kind as Collection>::Kind: Deserialize<'de2>,
        String: FromSql<ST, DB>,
        DB: Backend,
        DB: HasSqlType<ST>,
    {
        fn from_sql(
            raw: DB::RawValue<'_>,
        ) -> Result<Self, Box<dyn ::std::error::Error + Send + Sync>> {
            let string: String = FromSql::<ST, DB>::from_sql(raw)?;
            Ok(CollectionId::parse(&string)?)
        }
    }
    impl<Kind, ST, DB> Queryable<ST, DB> for CollectionId<Kind>
    where
        Kind: Collection + Send + 'static,
        for<'de2> <Kind as Collection>::Kind: Deserialize<'de2>,
        String: FromStaticSqlRow<ST, DB>,
        DB: Backend,
        DB: HasSqlType<ST>,
    {
        type Row = String;
        fn build(row: Self::Row) -> diesel::deserialize::Result<Self> {
            Ok(CollectionId::parse(&row)?)
        }
    }
    impl<Kind> QueryId for CollectionId<Kind>
    where
        Kind: Collection + 'static,
        for<'de2> <Kind as Collection>::Kind: Deserialize<'de2>,
    {
        type QueryId = Self;
    }
};
