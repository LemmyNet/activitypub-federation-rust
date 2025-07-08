use crate::{
    activities::create_post::CreatePost,
    database::DatabaseHandle,
    error::Error,
    generate_object_id,
    objects::person::DbUser,
};
use activitypub_federation::{
    config::Data,
    fetch::object_id::ObjectId,
    kinds::{object::NoteType, public},
    protocol::{helpers::deserialize_one_or_many, verification::verify_domains_match},
    traits::{Actor, Object},
};
use activitystreams_kinds::link::MentionType;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Clone, Debug)]
pub struct DbPost {
    pub text: String,
    pub ap_id: ObjectId<DbPost>,
    pub creator: ObjectId<DbUser>,
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
impl Object for DbPost {
    type DataType = DatabaseHandle;
    type Kind = Note;
    type Error = Error;

    fn id(&self) -> &Url {
        self.ap_id.inner()
    }

    async fn read_from_id(
        _object_id: Url,
        _data: &Data<Self::DataType>,
    ) -> Result<Option<Self>, Self::Error> {
        Ok(None)
    }

    async fn into_json(self, _data: &Data<Self::DataType>) -> Result<Self::Kind, Self::Error> {
        Ok(Note {
            kind: NoteType::Note,
            id: self.ap_id,
            content: self.text,
            attributed_to: self.creator,
            to: vec![public()],
            tag: vec![],
            in_reply_to: None,
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
        println!(
            "Received post with content {} and id {}",
            &json.content, &json.id
        );
        let creator = json.attributed_to.dereference(data).await?;
        let post = DbPost {
            text: json.content,
            ap_id: json.id.clone(),
            creator: json.attributed_to.clone(),
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
            in_reply_to: Some(json.id.clone()),
            tag: vec![mention],
        };
        CreatePost::send(note, creator.shared_inbox_or_inbox(), data).await?;

        Ok(post)
    }
}
