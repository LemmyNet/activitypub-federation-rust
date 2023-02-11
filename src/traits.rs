use crate::request_data::RequestData;
use chrono::NaiveDateTime;
use std::ops::Deref;
use url::Url;

/// Trait which allows verification and reception of incoming activities.
#[async_trait::async_trait]
#[enum_delegate::register]
pub trait ActivityHandler {
    type DataType: Send + Sync;
    type Error;

    /// `id` field of the activity
    fn id(&self) -> &Url;

    /// `actor` field of activity
    fn actor(&self) -> &Url;

    /// Receives the activity and stores its action in database.
    async fn receive(self, data: &RequestData<Self::DataType>) -> Result<(), Self::Error>;
}

/// Allow for boxing of enum variants
#[async_trait::async_trait]
impl<T> ActivityHandler for Box<T>
where
    T: ActivityHandler + Send,
{
    type DataType = T::DataType;
    type Error = T::Error;

    fn id(&self) -> &Url {
        self.deref().id()
    }

    fn actor(&self) -> &Url {
        self.deref().actor()
    }

    async fn receive(self, data: &RequestData<Self::DataType>) -> Result<(), Self::Error> {
        (*self).receive(data).await
    }
}

#[async_trait::async_trait]
pub trait ApubObject {
    type DataType: Send + Sync;
    type ApubType;
    type DbType;
    type Error;

    /// If the object is stored in the database, this method should return the fetch time. Used to
    /// update actors after certain interval.
    fn last_refreshed_at(&self) -> Option<NaiveDateTime> {
        None
    }

    /// Try to read the object with given ID from local database. Returns Ok(None) if it doesn't exist.
    async fn read_from_apub_id(
        object_id: Url,
        data: &RequestData<Self::DataType>,
    ) -> Result<Option<Self>, Self::Error>
    where
        Self: Sized;

    /// Marks the object as deleted in local db. Called when a delete activity is received, or if
    /// fetch returns a tombstone.
    async fn delete(self, _data: &RequestData<Self::DataType>) -> Result<(), Self::Error>
    where
        Self: Sized,
    {
        Ok(())
    }

    /// Trait for converting an object or actor into the respective ActivityPub type.
    async fn into_apub(
        self,
        data: &RequestData<Self::DataType>,
    ) -> Result<Self::ApubType, Self::Error>;

    /// Converts an object from ActivityPub type to Lemmy internal type.
    ///
    /// * `apub` The object to read from
    /// * `context` LemmyContext which holds DB pool, HTTP client etc
    /// * `expected_domain` Domain where the object was received from. None in case of mod action.
    /// * `mod_action_allowed` True if the object can be a mod activity, ignore `expected_domain` in this case
    async fn from_apub(
        apub: Self::ApubType,
        data: &RequestData<Self::DataType>,
    ) -> Result<Self, Self::Error>
    where
        Self: Sized;
}

pub trait Actor: ApubObject {
    /// Returns the actor's public key for verification of HTTP signatures
    fn public_key(&self) -> &str;

    /// The inbox where activities for this user should be sent to
    fn inbox(&self) -> Url;

    /// The actor's shared inbox, if any
    fn shared_inbox(&self) -> Option<Url> {
        None
    }

    fn shared_inbox_or_inbox(&self) -> Url {
        self.shared_inbox().unwrap_or_else(|| self.inbox())
    }
}
