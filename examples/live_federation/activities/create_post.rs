use crate::{
    database::DatabaseHandle,
    error::Error,
    objects::{person::DbUser, post::Note},
    utils::generate_object_id,
    DbPost,
};
use activitypub_federation::{
    activity_sending::SendActivityTask,
    config::Data,
    fetch::object_id::ObjectId,
    kinds::activity::CreateType,
    protocol::{context::WithContext, helpers::deserialize_one_or_many},
    traits::{ActivityHandler, Object},
    url::Url,
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CreatePost {
    pub(crate) actor: ObjectId<DbUser>,
    #[serde(deserialize_with = "deserialize_one_or_many")]
    pub(crate) to: Vec<Url>,
    pub(crate) object: Note,
    #[serde(rename = "type")]
    pub(crate) kind: CreateType,
    pub(crate) id: Url,
}

impl CreatePost {
    pub async fn send(note: Note, inbox: Url, data: &Data<DatabaseHandle>) -> Result<(), Error> {
        print!("Sending reply to {}", &note.attributed_to);
        let create = CreatePost {
            actor: note.attributed_to.clone(),
            to: note.to.clone(),
            object: note,
            kind: CreateType::Create,
            id: generate_object_id(data.domain())?,
        };
        let create_with_context = WithContext::new_default(create);
        let sends =
            SendActivityTask::prepare(&create_with_context, &data.local_user(), vec![inbox], data)
                .await?;
        for send in sends {
            send.sign_and_send(data).await?;
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl ActivityHandler for CreatePost {
    type DataType = DatabaseHandle;
    type Error = crate::error::Error;

    fn id(&self) -> &Url {
        &self.id
    }

    fn actor(&self) -> &Url {
        self.actor.inner()
    }

    async fn verify(&self, data: &Data<Self::DataType>) -> Result<(), Self::Error> {
        DbPost::verify(&self.object, &self.id, data).await?;
        Ok(())
    }

    async fn receive(self, data: &Data<Self::DataType>) -> Result<(), Self::Error> {
        DbPost::from_json(self.object, data).await?;
        Ok(())
    }
}
