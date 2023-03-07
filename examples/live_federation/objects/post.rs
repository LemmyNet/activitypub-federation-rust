use crate::{
    activities::create_post::CreatePost,
    database::DatabaseHandle,
    error::Error,
    generate_object_id,
    objects::person::DbUser,
};
use activitypub_federation::{
    config::RequestData,
    fetch::object_id::ObjectId,
    kinds::{object::NoteType, public},
    protocol::helpers::deserialize_one_or_many,
    traits::{Actor, ApubObject},
};
use activitystreams_kinds::link::MentionType;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Clone, Debug)]
pub struct DbPost {
    pub text: String,
    pub ap_id: ObjectId<DbPost>,
    pub creator: ObjectId<DbUser>,
    pub local: bool,
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
    in_reply_to: Option<ObjectId<DbPost>>,
    tag: Vec<Mention>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Mention {
    pub href: Url,
    #[serde(rename = "type")]
    pub kind: MentionType,
}

#[async_trait::async_trait]
impl ApubObject for DbPost {
    type DataType = DatabaseHandle;
    type ApubType = Note;
    type Error = Error;

    async fn read_from_apub_id(
        _object_id: Url,
        _data: &RequestData<Self::DataType>,
    ) -> Result<Option<Self>, Self::Error> {
        Ok(None)
    }

    async fn into_apub(
        self,
        _data: &RequestData<Self::DataType>,
    ) -> Result<Self::ApubType, Self::Error> {
        unimplemented!()
    }

    async fn from_apub(
        apub: Self::ApubType,
        data: &RequestData<Self::DataType>,
    ) -> Result<Self, Self::Error> {
        println!(
            "Received post with content {} and id {}",
            &apub.content, &apub.id
        );
        let creator = apub.attributed_to.dereference(data).await?;
        let post = DbPost {
            text: apub.content,
            ap_id: apub.id.clone(),
            creator: apub.attributed_to.clone(),
            local: false,
        };

        let mention = Mention {
            href: creator.ap_id.clone().into_inner(),
            kind: Default::default(),
        };
        let note = Note {
            kind: Default::default(),
            id: generate_object_id(data.domain())?.into(),
            attributed_to: data.local_user().ap_id,
            to: vec![public()],
            content: format!("Hello {}", creator.name),
            in_reply_to: Some(apub.id.clone()),
            tag: vec![mention],
        };
        CreatePost::send(note, creator.shared_inbox_or_inbox(), data).await?;

        Ok(post)
    }
}
