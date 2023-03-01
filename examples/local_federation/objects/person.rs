use crate::{
    activities::{accept::Accept, create_post::CreateNote, follow::Follow},
    error::Error,
    instance::DatabaseHandle,
    objects::post::DbPost,
    utils::generate_object_id,
};
use activitypub_federation::{
    config::RequestData,
    core::{
        activity_queue::send_activity,
        http_signatures::generate_actor_keypair,
        object_id::ObjectId,
    },
    kinds::actor::PersonType,
    protocol::{context::WithContext, public_key::PublicKey},
    traits::{ActivityHandler, Actor, ApubObject},
    webfinger::webfinger_resolve_actor,
};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use url::Url;

#[derive(Debug, Clone)]
pub struct DbUser {
    pub name: String,
    pub ap_id: ObjectId<DbUser>,
    pub inbox: Url,
    // exists for all users (necessary to verify http signatures)
    public_key: String,
    // exists only for local users
    private_key: Option<String>,
    pub followers: Vec<Url>,
    pub local: bool,
}

/// List of all activities which this actor can receive.
#[derive(Deserialize, Serialize, Debug)]
#[serde(untagged)]
#[enum_delegate::implement(ActivityHandler)]
pub enum PersonAcceptedActivities {
    Follow(Follow),
    Accept(Accept),
    CreateNote(CreateNote),
}

impl DbUser {
    pub fn new(hostname: &str, name: String) -> Result<DbUser, Error> {
        let ap_id = Url::parse(&format!("http://{}/{}", hostname, &name))?.into();
        let inbox = Url::parse(&format!("http://{}/{}/inbox", hostname, &name))?;
        let keypair = generate_actor_keypair()?;
        Ok(DbUser {
            name,
            ap_id,
            inbox,
            public_key: keypair.public_key,
            private_key: Some(keypair.private_key),
            followers: vec![],
            local: true,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    #[serde(rename = "type")]
    kind: PersonType,
    preferred_username: String,
    id: ObjectId<DbUser>,
    inbox: Url,
    public_key: PublicKey,
}

impl DbUser {
    pub fn followers(&self) -> &Vec<Url> {
        &self.followers
    }

    pub fn followers_url(&self) -> Result<Url, Error> {
        Ok(Url::parse(&format!("{}/followers", self.ap_id.inner()))?)
    }

    fn public_key(&self) -> PublicKey {
        PublicKey::new(self.ap_id.clone().into_inner(), self.public_key.clone())
    }

    pub async fn follow(
        &self,
        other: &str,
        data: &RequestData<DatabaseHandle>,
    ) -> Result<(), Error> {
        let other: DbUser = webfinger_resolve_actor(other, data).await?;
        let id = generate_object_id(data.domain())?;
        let follow = Follow::new(self.ap_id.clone(), other.ap_id.clone(), id.clone());
        self.send(follow, vec![other.shared_inbox_or_inbox()], data)
            .await?;
        Ok(())
    }

    pub async fn post(
        &self,
        post: DbPost,
        data: &RequestData<DatabaseHandle>,
    ) -> Result<(), Error> {
        let id = generate_object_id(data.domain())?;
        let create = CreateNote::new(post.into_apub(data).await?, id.clone());
        let mut inboxes = vec![];
        for f in self.followers.clone() {
            let user: DbUser = ObjectId::from(f).dereference(data).await?;
            inboxes.push(user.shared_inbox_or_inbox());
        }
        self.send(create, inboxes, data).await?;
        Ok(())
    }

    pub(crate) async fn send<Activity>(
        &self,
        activity: Activity,
        recipients: Vec<Url>,
        data: &RequestData<DatabaseHandle>,
    ) -> Result<(), <Activity as ActivityHandler>::Error>
    where
        Activity: ActivityHandler + Serialize + Debug + Send + Sync,
        <Activity as ActivityHandler>::Error: From<anyhow::Error> + From<serde_json::Error>,
    {
        let activity = WithContext::new_default(activity);
        send_activity(
            activity,
            self.private_key.clone().unwrap(),
            recipients,
            data,
        )
        .await?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl ApubObject for DbUser {
    type DataType = DatabaseHandle;
    type ApubType = Person;
    type Error = Error;

    async fn read_from_apub_id(
        object_id: Url,
        data: &RequestData<Self::DataType>,
    ) -> Result<Option<Self>, Self::Error> {
        let users = data.users.lock().unwrap();
        let res = users
            .clone()
            .into_iter()
            .find(|u| u.ap_id.inner() == &object_id);
        Ok(res)
    }

    async fn into_apub(
        self,
        _data: &RequestData<Self::DataType>,
    ) -> Result<Self::ApubType, Self::Error> {
        Ok(Person {
            preferred_username: self.name.clone(),
            kind: Default::default(),
            id: self.ap_id.clone(),
            inbox: self.inbox.clone(),
            public_key: self.public_key(),
        })
    }

    async fn from_apub(
        apub: Self::ApubType,
        _data: &RequestData<Self::DataType>,
    ) -> Result<Self, Self::Error> {
        Ok(DbUser {
            name: apub.preferred_username,
            ap_id: apub.id,
            inbox: apub.inbox,
            public_key: apub.public_key.public_key_pem,
            private_key: None,
            followers: vec![],
            local: false,
        })
    }
}

impl Actor for DbUser {
    fn public_key(&self) -> &str {
        &self.public_key
    }

    fn inbox(&self) -> Url {
        self.inbox.clone()
    }
}
