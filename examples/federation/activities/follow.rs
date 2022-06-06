use crate::{
    activities::accept::Accept,
    generate_object_id,
    instance::InstanceHandle,
    objects::person::MyUser,
};
use activitypub_federation::{
    core::object_id::ObjectId,
    data::Data,
    traits::{ActivityHandler, Actor},
};
use activitystreams_kinds::activity::FollowType;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Follow {
    pub(crate) actor: ObjectId<MyUser>,
    pub(crate) object: ObjectId<MyUser>,
    #[serde(rename = "type")]
    kind: FollowType,
    id: Url,
}

impl Follow {
    pub fn new(actor: ObjectId<MyUser>, object: ObjectId<MyUser>, id: Url) -> Follow {
        Follow {
            actor,
            object,
            kind: Default::default(),
            id,
        }
    }
}

#[async_trait::async_trait(?Send)]
impl ActivityHandler for Follow {
    type DataType = InstanceHandle;
    type Error = crate::error::Error;

    fn id(&self) -> &Url {
        &self.id
    }

    fn actor(&self) -> &Url {
        self.actor.inner()
    }

    async fn verify(
        &self,
        _data: &Data<Self::DataType>,
        _request_counter: &mut i32,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    // Ignore clippy false positive: https://github.com/rust-lang/rust-clippy/issues/6446
    #[allow(clippy::await_holding_lock)]
    async fn receive(
        self,
        data: &Data<Self::DataType>,
        request_counter: &mut i32,
    ) -> Result<(), Self::Error> {
        // add to followers
        let mut users = data.users.lock().unwrap();
        let local_user = users.first_mut().unwrap();
        local_user.followers.push(self.actor.inner().clone());
        let local_user = local_user.clone();
        drop(users);

        // send back an accept
        let follower = self
            .actor
            .dereference(data, data.local_instance(), request_counter)
            .await?;
        let id = generate_object_id(data.local_instance().hostname())?;
        let accept = Accept::new(local_user.ap_id.clone(), self, id.clone());
        local_user
            .send(
                accept,
                vec![follower.shared_inbox_or_inbox()],
                data.local_instance(),
            )
            .await?;
        Ok(())
    }
}
