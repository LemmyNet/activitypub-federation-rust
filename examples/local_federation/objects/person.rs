use crate::{
    activities::{accept::Accept, create_post::CreatePost, follow::Follow},
    error::Error,
    instance::DatabaseHandle,
    objects::post::DbPost,
    utils::generate_object_id,
};
use activitypub_federation::{
    activity_queue::queue_activity,
    activity_sending::SendActivityTask,
    config::Data,
    fetch::{object_id::ObjectId, webfinger::webfinger_resolve_actor},
    http_signatures::generate_actor_keypair,
    kinds::actor::PersonType,
    protocol::{context::WithContext, public_key::PublicKey, verification::verify_domains_match},
    traits::{ActivityHandler, Actor, Object},
};
use chrono::{DateTime, Utc};
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
    last_refreshed_at: DateTime<Utc>,
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
    CreateNote(CreatePost),
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
            last_refreshed_at: Utc::now(),
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

    pub async fn follow(&self, other: &str, data: &Data<DatabaseHandle>) -> Result<(), Error> {
        let other: DbUser = webfinger_resolve_actor(other, data).await?;
        let id = generate_object_id(data.domain())?;
        let follow = Follow::new(self.ap_id.clone(), other.ap_id.clone(), id.clone());
        self.send(follow, vec![other.shared_inbox_or_inbox()], false, data)
            .await?;
        Ok(())
    }

    pub async fn post(&self, post: DbPost, data: &Data<DatabaseHandle>) -> Result<(), Error> {
        let id = generate_object_id(data.domain())?;
        let create = CreatePost::new(post.into_json(data).await?, id.clone());
        let mut inboxes = vec![];
        for f in self.followers.clone() {
            let user: DbUser = ObjectId::from(f).dereference(data).await?;
            inboxes.push(user.shared_inbox_or_inbox());
        }
        self.send(create, inboxes, true, data).await?;
        Ok(())
    }

    pub(crate) async fn send<Activity>(
        &self,
        activity: Activity,
        recipients: Vec<Url>,
        use_queue: bool,
        data: &Data<DatabaseHandle>,
    ) -> Result<(), Error>
    where
        Activity: ActivityHandler + Serialize + Debug + Send + Sync,
        <Activity as ActivityHandler>::Error: From<anyhow::Error> + From<serde_json::Error>,
    {
        let activity = WithContext::new_default(activity);
        // Send through queue in some cases and bypass it in others to test both code paths
        if use_queue {
            queue_activity(activity, self, recipients, data).await?;
        } else {
            let sends = SendActivityTask::prepare(&activity, self, recipients, data).await?;
            for send in sends {
                send.sign_and_send(data).await?;
            }
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl Object for DbUser {
    type DataType = DatabaseHandle;
    type Kind = Person;
    type Error = Error;

    fn last_refreshed_at(&self) -> Option<DateTime<Utc>> {
        Some(self.last_refreshed_at)
    }

    async fn read_from_id(
        object_id: Url,
        data: &Data<Self::DataType>,
    ) -> Result<Option<Self>, Self::Error> {
        let users = data.users.lock().unwrap();
        let res = users
            .clone()
            .into_iter()
            .find(|u| u.ap_id.inner() == &object_id);
        Ok(res)
    }

    async fn into_json(self, _data: &Data<Self::DataType>) -> Result<Self::Kind, Self::Error> {
        Ok(Person {
            preferred_username: self.name.clone(),
            kind: Default::default(),
            id: self.ap_id.clone(),
            inbox: self.inbox.clone(),
            public_key: self.public_key(),
        })
    }

    async fn verify(
        json: &Self::Kind,
        expected_domain: &Url,
        _data: &Data<Self::DataType>,
    ) -> Result<(), Self::Error> {
        verify_domains_match(json.id.inner(), expected_domain)?;
        Ok(())
    }

    async fn from_json(json: Self::Kind, data: &Data<Self::DataType>) -> Result<Self, Self::Error> {
        let user = DbUser {
            name: json.preferred_username,
            ap_id: json.id,
            inbox: json.inbox,
            public_key: json.public_key.public_key_pem,
            private_key: None,
            last_refreshed_at: Utc::now(),
            followers: vec![],
            local: false,
        };
        let mut mutex = data.users.lock().unwrap();
        mutex.push(user.clone());
        Ok(user)
    }
}

impl Actor for DbUser {
    fn id(&self) -> Url {
        self.ap_id.inner().clone()
    }

    fn public_key_pem(&self) -> &str {
        &self.public_key
    }

    fn private_key_pem(&self) -> Option<String> {
        self.private_key.clone()
    }

    fn inbox(&self) -> Url {
        self.inbox.clone()
    }
}
