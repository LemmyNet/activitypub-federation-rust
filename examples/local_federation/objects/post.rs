use crate::{error::Error, generate_object_id, instance::DatabaseHandle, objects::person::DbUser};
use activitypub_federation::{
    config::Data,
    fetch::object_id::ObjectId,
    kinds::{object::NoteType, public},
    protocol::{helpers::deserialize_one_or_many, verification::verify_domains_match},
    traits::Object,
    url::Url,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct DbPost {
    pub text: String,
    pub ap_id: ObjectId<DbPost>,
    pub creator: ObjectId<DbUser>,
    pub local: bool,
}

impl DbPost {
    pub fn new(text: String, creator: ObjectId<DbUser>) -> Result<DbPost, Error> {
        let ap_id = generate_object_id(creator.inner().domain())?.into();
        Ok(DbPost {
            text,
            ap_id,
            creator,
            local: true,
        })
    }
}

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    #[serde(rename = "type")]
    kind: NoteType,
    id: ObjectId<DbPost>,
    pub(crate) attributed_to: ObjectId<DbUser>,
    #[serde(deserialize_with = "deserialize_one_or_many")]
    pub(crate) to: Vec<Url>,
    content: String,
}

#[async_trait::async_trait]
impl Object for DbPost {
    type DataType = DatabaseHandle;
    type Kind = Note;
    type Error = Error;

    async fn read_from_id(
        object_id: Url,
        data: &Data<Self::DataType>,
    ) -> Result<Option<Self>, Self::Error> {
        let posts = data.posts.lock().unwrap();
        let res = posts
            .clone()
            .into_iter()
            .find(|u| u.ap_id.inner() == &object_id);
        Ok(res)
    }

    async fn into_json(self, data: &Data<Self::DataType>) -> Result<Self::Kind, Self::Error> {
        let creator = self.creator.dereference_local(data).await?;
        Ok(Note {
            kind: Default::default(),
            id: self.ap_id,
            attributed_to: self.creator,
            to: vec![public().try_into()?, creator.followers_url()?],
            content: self.text,
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
        let post = DbPost {
            text: json.content,
            ap_id: json.id,
            creator: json.attributed_to,
            local: false,
        };

        let mut lock = data.posts.lock().unwrap();
        lock.push(post.clone());
        Ok(post)
    }
}
