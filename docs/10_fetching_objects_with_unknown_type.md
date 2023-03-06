## Fetching remote object with unknown type

It is sometimes necessary to fetch from a URL, but we don't know the exact type of object it will return. An example is the search field in most federated platforms, which allows pasting and `id` URL and fetches it from the origin server. It can be implemented in the following way:

```no_run
# use activitypub_federation::traits::tests::{DbUser, DbPost};
# use activitypub_federation::fetch::object_id::ObjectId;
# use activitypub_federation::traits::ApubObject;
# use activitypub_federation::config::FederationConfig;
# use serde::{Deserialize, Serialize};
# use activitypub_federation::traits::tests::DbConnection;
# use activitypub_federation::config::RequestData;
# use url::Url;
# use activitypub_federation::traits::tests::{Person, Note};

pub enum SearchableDbObjects {
    User(DbUser),
    Post(DbPost)
}

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
pub enum SearchableApubObjects {
    Person(Person),
    Note(Note)
}

#[async_trait::async_trait]
impl ApubObject for SearchableDbObjects {
    type DataType = DbConnection;
    type ApubType = SearchableApubObjects;
    type Error = anyhow::Error;

    async fn read_from_apub_id(
        object_id: Url,
        data: &RequestData<Self::DataType>,
    ) -> Result<Option<Self>, Self::Error> {
        Ok(None)
    }

    async fn into_apub(
        self,
        data: &RequestData<Self::DataType>,
    ) -> Result<Self::ApubType, Self::Error> {
        unimplemented!();
    }

    async fn from_apub(
        apub: Self::ApubType,
        data: &RequestData<Self::DataType>,
    ) -> Result<Self, Self::Error> {
        use SearchableDbObjects::*;
        match apub {
            SearchableApubObjects::Person(p) => Ok(User(DbUser::from_apub(p, data).await?)),
            SearchableApubObjects::Note(n) => Ok(Post(DbPost::from_apub(n, data).await?)),
        }
    }
}

#[actix_rt::main]
async fn main() -> Result<(), anyhow::Error> {
    # let config = FederationConfig::builder().domain("example.com").app_data(DbConnection).build().unwrap();
    # let data = config.to_request_data();
    let query = "https://example.com/id/413";
    let query_result = ObjectId::<SearchableDbObjects>::new(query)?
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
