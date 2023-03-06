## Sending and receiving activities

Activitypub propagates actions across servers using `Activities`. For this each actor has an inbox and a public/private key pair. We already defined a `Person` actor with keypair. Whats left is to define an activity. This is similar to the way we defined `Person` and `Note` structs before. In this case we need to implement the [ActivityHandler](trait@crate::traits::ActivityHandler) trait.

```
# use serde::{Deserialize, Serialize};
# use url::Url;
# use anyhow::Error;
# use async_trait::async_trait;
# use activitypub_federation::fetch::object_id::ObjectId;
# use activitypub_federation::traits::tests::{DbConnection, DbUser};
# use activitystreams_kinds::activity::FollowType;
# use activitypub_federation::traits::ActivityHandler;
# use activitypub_federation::config::RequestData;
# async fn send_accept() -> Result<(), Error> { Ok(()) }

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Follow {
    pub actor: ObjectId<DbUser>,
    pub object: ObjectId<DbUser>,
    #[serde(rename = "type")]
    pub kind: FollowType,
    pub id: Url,
}

#[async_trait]
impl ActivityHandler for Follow {
    type DataType = DbConnection;
    type Error = Error;

    fn id(&self) -> &Url {
        &self.id
    }

    fn actor(&self) -> &Url {
        self.actor.inner()
    }

    async fn receive(self, data: &RequestData<Self::DataType>) -> Result<(), Self::Error> {
        let actor = self.actor.dereference(data).await?;
        let followed = self.object.dereference(data).await?;
        data.add_follower(followed, actor).await?;
        Ok(())
    }
}
```

In this case there is no need to convert to a database type, because activities don't need to be stored in the database in full. Instead we dereference the involved user accounts, and create a follow relation in the database.

Next its time to setup the actual HTTP handler for the inbox. For this we first define an enum of all activities which are accepted by the actor. Then we just need to define an HTTP endpoint at the path of our choice (identical to `Person.inbox` defined earlier). This endpoint needs to hand received data over to [receive_activity](crate::axum::inbox::receive_activity). This method verifies the HTTP signature, checks the blocklist with [FederationConfigBuilder::url_verifier](crate::config::FederationConfigBuilder::url_verifier) and more. If everything is valid, the activity is passed to the `receive` method we defined above.

```
# use axum::response::IntoResponse;
# use activitypub_federation::axum::inbox::{ActivityData, receive_activity};
# use activitypub_federation::config::RequestData;
# use activitypub_federation::protocol::context::WithContext;
# use activitypub_federation::traits::ActivityHandler;
# use activitypub_federation::traits::tests::{DbConnection, DbUser, Follow};
# use serde::{Deserialize, Serialize};
# use url::Url;

#[derive(Deserialize, Serialize, Debug)]
#[serde(untagged)]
#[enum_delegate::implement(ActivityHandler)]
pub enum PersonAcceptedActivities {
    Follow(Follow),
}

async fn http_post_user_inbox(
    data: RequestData<DbConnection>,
    activity_data: ActivityData,
) -> impl IntoResponse {
    receive_activity::<WithContext<PersonAcceptedActivities>, DbUser, DbConnection>(
        activity_data,
        &data,
    )
        .await.unwrap()
}
```

The `PersonAcceptedActivities` works by attempting to parse the received JSON data with each variant in order. The first variant which parses without errors is used for receiving. This means you should avoid defining multiple activities in a way that they might conflict and parse the same data.

Activity enums can also be nested. 
