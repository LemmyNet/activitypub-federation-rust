use crate::{error::Error, generate_object_id, instance::DatabaseHandle, objects::person::DbUser};
use activitypub_federation::{
    config::RequestData,
    fetch::object_id::ObjectId,
    kinds::{object::NoteType, public},
    protocol::helpers::deserialize_one_or_many,
    traits::ApubObject,
};
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Clone, Debug)]
pub struct DbPost {
    pub text: String,
    pub ap_id: ObjectId<DbPost>,
    pub creator: ObjectId<DbUser>,
    pub local: bool,
}

impl DbPost {
    pub fn new(text: String, creator: ObjectId<DbUser>) -> Result<DbPost, Error> {
        let ap_id = generate_object_id(creator.inner().domain().unwrap())?.into();
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
impl ApubObject for DbPost {
    type DataType = DatabaseHandle;
    type ApubType = Note;
    type Error = Error;

    async fn read_from_apub_id(
        object_id: Url,
        data: &RequestData<Self::DataType>,
    ) -> Result<Option<Self>, Self::Error> {
        let posts = data.posts.lock().unwrap();
        let res = posts
            .clone()
            .into_iter()
            .find(|u| u.ap_id.inner() == &object_id);
        Ok(res)
    }

    async fn into_apub(
        self,
        data: &RequestData<Self::DataType>,
    ) -> Result<Self::ApubType, Self::Error> {
        let creator = self.creator.dereference_local(data).await?;
        Ok(Note {
            kind: Default::default(),
            id: self.ap_id,
            attributed_to: self.creator,
            to: vec![public(), creator.followers_url()?],
            content: self.text,
        })
    }

    async fn from_apub(
        apub: Self::ApubType,
        data: &RequestData<Self::DataType>,
    ) -> Result<Self, Self::Error> {
        let post = DbPost {
            text: apub.content,
            ap_id: apub.id,
            creator: apub.attributed_to,
            local: false,
        };

        let mut lock = data.posts.lock().unwrap();
        lock.push(post.clone());
        Ok(post)
    }
}
