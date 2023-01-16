use crate::{
    activities::{accept::Accept, create_note::CreateNote, follow::Follow},
    error::Error,
    instance::InstanceHandle,
    objects::note::MyPost,
    utils::generate_object_id,
};
use activitypub_federation::{
    core::{
        activity_queue::send_activity,
        object_id::ObjectId,
        signatures::{Keypair, PublicKey},
    },
    data::Data,
    deser::context::WithContext,
    traits::{ActivityHandler, Actor, ApubObject},
    LocalInstance,
};
use activitystreams_kinds::actor::PersonType;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone)]
pub struct MyUser {
    pub ap_id: ObjectId<MyUser>,
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

impl MyUser {
    pub fn new(ap_id: Url, keypair: Keypair) -> MyUser {
        let mut inbox = ap_id.clone();
        inbox.set_path("/inbox");
        let ap_id = ObjectId::new(ap_id);
        MyUser {
            ap_id,
            inbox,
            public_key: keypair.public_key,
            private_key: Some(keypair.private_key),
            followers: vec![],
            local: true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    #[serde(rename = "type")]
    kind: PersonType,
    id: ObjectId<MyUser>,
    inbox: Url,
    public_key: PublicKey,
}

impl MyUser {
    pub fn followers(&self) -> &Vec<Url> {
        &self.followers
    }

    pub fn followers_url(&self) -> Result<Url, Error> {
        Ok(Url::parse(&format!("{}/followers", self.ap_id.inner()))?)
    }

    fn public_key(&self) -> PublicKey {
        PublicKey::new_main_key(self.ap_id.clone().into_inner(), self.public_key.clone())
    }

    pub async fn follow(&self, other: &MyUser, instance: &InstanceHandle) -> Result<(), Error> {
        let id = generate_object_id(instance.local_instance().hostname())?;
        let follow = Follow::new(self.ap_id.clone(), other.ap_id.clone(), id.clone());
        self.send(
            follow,
            vec![other.shared_inbox_or_inbox()],
            instance.local_instance(),
        )
        .await?;
        Ok(())
    }

    pub async fn post(&self, post: MyPost, instance: &InstanceHandle) -> Result<(), Error> {
        let id = generate_object_id(instance.local_instance().hostname())?;
        let create = CreateNote::new(post.into_apub(instance).await?, id.clone());
        let mut inboxes = vec![];
        for f in self.followers.clone() {
            let user: MyUser = ObjectId::new(f)
                .dereference(instance, instance.local_instance(), &mut 0)
                .await?;
            inboxes.push(user.shared_inbox_or_inbox());
        }
        self.send(create, inboxes, instance.local_instance())
            .await?;
        Ok(())
    }

    pub(crate) async fn send<Activity>(
        &self,
        activity: Activity,
        recipients: Vec<Url>,
        local_instance: &LocalInstance,
    ) -> Result<(), <Activity as ActivityHandler>::Error>
    where
        Activity: ActivityHandler + Serialize + Send + Sync,
        <Activity as ActivityHandler>::Error: From<anyhow::Error> + From<serde_json::Error>,
    {
        let activity = WithContext::new_default(activity);
        send_activity(
            activity,
            self.public_key(),
            self.private_key.clone().expect("has private key"),
            recipients,
            local_instance,
        )
        .await?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl ApubObject for MyUser {
    type DataType = InstanceHandle;
    type ApubType = Person;
    type DbType = MyUser;
    type Error = crate::error::Error;

    async fn read_from_apub_id(
        object_id: Url,
        data: &Self::DataType,
    ) -> Result<Option<Self>, Self::Error> {
        let users = data.users.lock().unwrap();
        let res = users
            .clone()
            .into_iter()
            .find(|u| u.ap_id.inner() == &object_id);
        Ok(res)
    }

    async fn into_apub(self, _data: &Self::DataType) -> Result<Self::ApubType, Self::Error> {
        Ok(Person {
            kind: Default::default(),
            id: self.ap_id.clone(),
            inbox: self.inbox.clone(),
            public_key: self.public_key(),
        })
    }

    async fn from_apub(
        apub: Self::ApubType,
        _data: &Self::DataType,
        _request_counter: &mut i32,
    ) -> Result<Self, Self::Error> {
        Ok(MyUser {
            ap_id: apub.id,
            inbox: apub.inbox,
            public_key: apub.public_key.public_key_pem,
            private_key: None,
            followers: vec![],
            local: false,
        })
    }
}

impl Actor for MyUser {
    fn public_key(&self) -> &str {
        &self.public_key
    }

    fn inbox(&self) -> Url {
        self.inbox.clone()
    }
}
