use crate::{
    instance::DatabaseHandle,
    objects::{person::DbUser, post::Note},
    DbPost,
};
use activitypub_federation::{
    config::RequestData,
    core::object_id::ObjectId,
    kinds::activity::CreateType,
    protocol::helpers::deserialize_one_or_many,
    traits::{ActivityHandler, ApubObject},
};
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CreateNote {
    pub(crate) actor: ObjectId<DbUser>,
    #[serde(deserialize_with = "deserialize_one_or_many")]
    pub(crate) to: Vec<Url>,
    pub(crate) object: Note,
    #[serde(rename = "type")]
    pub(crate) kind: CreateType,
    pub(crate) id: Url,
}

impl CreateNote {
    pub fn new(note: Note, id: Url) -> CreateNote {
        CreateNote {
            actor: note.attributed_to.clone(),
            to: note.to.clone(),
            object: note,
            kind: CreateType::Create,
            id,
        }
    }
}

#[async_trait::async_trait]
impl ActivityHandler for CreateNote {
    type DataType = DatabaseHandle;
    type Error = crate::error::Error;

    fn id(&self) -> &Url {
        &self.id
    }

    fn actor(&self) -> &Url {
        self.actor.inner()
    }

    async fn receive(self, data: &RequestData<Self::DataType>) -> Result<(), Self::Error> {
        DbPost::from_apub(self.object, data).await?;
        Ok(())
    }
}
