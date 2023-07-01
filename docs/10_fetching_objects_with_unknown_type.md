## Fetching remote object with unknown type

It is sometimes necessary to fetch from a URL, but we don't know the exact type of object it will return. An example is the search field in most federated platforms, which allows pasting and `id` URL and fetches it from the origin server. It can be implemented in the following way:

```no_run
# use activitypub_federation::traits::tests::{DbUser, DbPost};
# use activitypub_federation::fetch::object_id::ObjectId;
# use activitypub_federation::traits::Object;
# use activitypub_federation::config::FederationConfig;
# use serde::{Deserialize, Serialize};
# use activitypub_federation::traits::tests::DbConnection;
# use activitypub_federation::config::Data;
# use url::Url;
# use std::sync::Arc;
# use activitypub_federation::traits::tests::{Person, Note};

#[derive(Clone)]
pub enum SearchableDbObjects {
    User(DbUser),
    Post(DbPost)
}

#[derive(Deserialize, Serialize, Debug)]
#[serde(untagged)]
pub enum SearchableObjects {
    Person(Person),
    Note(Note)
}

#[derive(Debug, Clone)]
pub struct Error(pub(crate) Arc<anyhow::Error>);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

impl<T> From<T> for Error
where
    T: Into<anyhow::Error>,
{
    fn from(t: T) -> Self {
        Error(Arc::new(t.into()))
    }
}


#[async_trait::async_trait]
impl Object for SearchableDbObjects {
    type DataType = DbConnection;
    type Kind = SearchableObjects;
    type Error = Error;

    async fn read_from_id(
        object_id: Url,
        data: &Data<Self::DataType>,
    ) -> Result<Option<Self>, Self::Error> {
        Ok(None)
    }

    async fn into_json(
        self,
        data: &Data<Self::DataType>,
    ) -> Result<Self::Kind, Self::Error> {
        unimplemented!();
    }
    
    async fn verify(json: &Self::Kind, expected_domain: &Url, _data: &Data<Self::DataType>) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn from_json(
        json: Self::Kind,
        data: &Data<Self::DataType>,
    ) -> Result<Self, Self::Error> {
        use SearchableDbObjects::*;
        match json {
            SearchableObjects::Person(p) => Ok(User(DbUser::from_json(p, data).await?)),
            SearchableObjects::Note(n) => Ok(Post(DbPost::from_json(n, data).await?)),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    # let config = FederationConfig::builder().domain("example.com").app_data(DbConnection).build().await.unwrap();
    # let data = config.to_request_data();
    let query = "https://example.com/id/413";
    let query_result = ObjectId::<SearchableDbObjects>::parse(query)?
        .dereference(&data)
        .await?;
    match query_result {
        SearchableDbObjects::Post(post) => {} // retrieved object is a post
        SearchableDbObjects::User(user) => {} // object is a user
    };
    Ok(())
}
```

This is similar to the way receiving activities are handled in the previous section. The remote JSON is fetched, and received using the first enum variant which can successfully deserialize the data.
