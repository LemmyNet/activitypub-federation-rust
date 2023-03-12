use crate::{activities::create_post::CreatePost, database::DatabaseHandle, error::Error};
use activitypub_federation::{
    config::Data,
    fetch::object_id::ObjectId,
    http_signatures::generate_actor_keypair,
    kinds::actor::PersonType,
    protocol::{public_key::PublicKey, verification::verify_domains_match},
    traits::{ActivityHandler, Actor, ApubObject},
};
use chrono::{Local, NaiveDateTime};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use url::Url;

#[derive(Debug, Clone)]
pub struct DbUser {
    pub name: String,
    pub ap_id: ObjectId<DbUser>,
    pub inbox: Url,
    // exists for all users (necessary to verify http signatures)
    pub public_key: String,
    // exists only for local users
    pub private_key: Option<String>,
    last_refreshed_at: NaiveDateTime,
    pub followers: Vec<Url>,
    pub local: bool,
}

/// List of all activities which this actor can receive.
#[derive(Deserialize, Serialize, Debug)]
#[serde(untagged)]
#[enum_delegate::implement(ActivityHandler)]
pub enum PersonAcceptedActivities {
    CreateNote(CreatePost),
}

impl DbUser {
    pub fn new(hostname: &str, name: &str) -> Result<DbUser, Error> {
        let ap_id = Url::parse(&format!("https://{}/{}", hostname, &name))?.into();
        let inbox = Url::parse(&format!("https://{}/{}/inbox", hostname, &name))?;
        let keypair = generate_actor_keypair()?;
        Ok(DbUser {
            name: name.to_string(),
            ap_id,
            inbox,
            public_key: keypair.public_key,
            private_key: Some(keypair.private_key),
            last_refreshed_at: Local::now().naive_local(),
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

#[async_trait::async_trait]
impl ApubObject for DbUser {
    type DataType = DatabaseHandle;
    type ApubType = Person;
    type Error = Error;

    fn last_refreshed_at(&self) -> Option<NaiveDateTime> {
        Some(self.last_refreshed_at)
    }

    async fn read_from_apub_id(
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

    async fn into_apub(self, _data: &Data<Self::DataType>) -> Result<Self::ApubType, Self::Error> {
        Ok(Person {
            preferred_username: self.name.clone(),
            kind: Default::default(),
            id: self.ap_id.clone(),
            inbox: self.inbox.clone(),
            public_key: self.public_key(),
        })
    }

    async fn verify(
        apub: &Self::ApubType,
        expected_domain: &Url,
        _data: &Data<Self::DataType>,
    ) -> Result<(), Self::Error> {
        verify_domains_match(apub.id.inner(), expected_domain)?;
        Ok(())
    }

    async fn from_apub(
        apub: Self::ApubType,
        _data: &Data<Self::DataType>,
    ) -> Result<Self, Self::Error> {
        Ok(DbUser {
            name: apub.preferred_username,
            ap_id: apub.id,
            inbox: apub.inbox,
            public_key: apub.public_key.public_key_pem,
            private_key: None,
            last_refreshed_at: Local::now().naive_local(),
            followers: vec![],
            local: false,
        })
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
