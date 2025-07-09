#![doc(hidden)]
#![allow(clippy::unwrap_used)]
//! Some impls of these traits for use in tests. Dont use this from external crates.
//!
//! TODO: Should be using `cfg[doctest]` but blocked by <https://github.com/rust-lang/rust/issues/67295>

use super::{async_trait, Activity, Actor, Data, Debug, Object, PublicKey, Url};
use crate::{
    error::Error,
    fetch::object_id::ObjectId,
    http_signatures::{generate_actor_keypair, Keypair},
    protocol::verification::verify_domains_match,
};
use activitystreams_kinds::{activity::FollowType, actor::PersonType};
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

#[derive(Clone)]
pub struct DbConnection;

impl DbConnection {
    pub async fn read_post_from_json_id<T>(&self, _: Url) -> Result<Option<T>, Error> {
        Ok(None)
    }
    pub async fn read_local_user(&self, _: &str) -> Result<DbUser, Error> {
        todo!()
    }
    pub async fn upsert<T>(&self, _: &T) -> Result<(), Error> {
        Ok(())
    }
    pub async fn add_follower(&self, _: DbUser, _: DbUser) -> Result<(), Error> {
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    #[serde(rename = "type")]
    pub kind: PersonType,
    pub preferred_username: String,
    pub id: ObjectId<DbUser>,
    pub inbox: Url,
    pub public_key: PublicKey,
}
#[derive(Debug, Clone)]
pub struct DbUser {
    pub name: String,
    pub federation_id: Url,
    pub inbox: Url,
    pub public_key: String,
    #[allow(dead_code)]
    private_key: Option<String>,
    pub followers: Vec<Url>,
    pub local: bool,
}

pub static DB_USER_KEYPAIR: LazyLock<Keypair> = LazyLock::new(|| generate_actor_keypair().unwrap());

pub static DB_USER: LazyLock<DbUser> = LazyLock::new(|| DbUser {
    name: String::new(),
    federation_id: "https://localhost/123".parse().unwrap(),
    inbox: "https://localhost/123/inbox".parse().unwrap(),
    public_key: DB_USER_KEYPAIR.public_key.clone(),
    private_key: Some(DB_USER_KEYPAIR.private_key.clone()),
    followers: vec![],
    local: false,
});

#[async_trait]
impl Object for DbUser {
    type DataType = DbConnection;
    type Kind = Person;
    type Error = Error;

    fn id(&self) -> &Url {
        &self.federation_id
    }

    async fn read_from_id(
        _object_id: Url,
        _data: &Data<Self::DataType>,
    ) -> Result<Option<Self>, Self::Error> {
        Ok(Some(DB_USER.clone()))
    }

    async fn into_json(self, _data: &Data<Self::DataType>) -> Result<Self::Kind, Self::Error> {
        Ok(Person {
            preferred_username: self.name.clone(),
            kind: Default::default(),
            id: self.federation_id.clone().into(),
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

    async fn from_json(
        json: Self::Kind,
        _data: &Data<Self::DataType>,
    ) -> Result<Self, Self::Error> {
        Ok(DbUser {
            name: json.preferred_username,
            federation_id: json.id.into(),
            inbox: json.inbox,
            public_key: json.public_key.public_key_pem,
            private_key: None,
            followers: vec![],
            local: false,
        })
    }
}

impl Actor for DbUser {
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
impl Activity for Follow {
    type DataType = DbConnection;
    type Error = Error;

    fn id(&self) -> &Url {
        &self.id
    }

    fn actor(&self) -> &Url {
        self.actor.inner()
    }

    async fn verify(&self, _: &Data<Self::DataType>) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn receive(self, _data: &Data<Self::DataType>) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {}
#[derive(Debug, Clone)]
pub struct DbPost {
    pub federation_id: Url,
}

#[async_trait]
impl Object for DbPost {
    type DataType = DbConnection;
    type Kind = Note;
    type Error = Error;

    fn id(&self) -> &Url {
        todo!()
    }

    async fn read_from_id(_: Url, _: &Data<Self::DataType>) -> Result<Option<Self>, Self::Error> {
        todo!()
    }

    async fn into_json(self, _: &Data<Self::DataType>) -> Result<Self::Kind, Self::Error> {
        todo!()
    }

    async fn verify(_: &Self::Kind, _: &Url, _: &Data<Self::DataType>) -> Result<(), Self::Error> {
        todo!()
    }

    async fn from_json(_: Self::Kind, _: &Data<Self::DataType>) -> Result<Self, Self::Error> {
        todo!()
    }
}
