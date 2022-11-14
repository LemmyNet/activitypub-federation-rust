use crate::data::Data;
use chrono::NaiveDateTime;
use std::ops::Deref;
use url::Url;

/// Trait which allows verification and reception of incoming activities.
#[async_trait::async_trait(?Send)]
#[enum_delegate::register]
pub trait ActivityHandler {
    type DataType;
    type Error;

    /// `id` field of the activity
    fn id(&self) -> &Url;

    /// `actor` field of activity
    fn actor(&self) -> &Url;

    /// Verify that the activity is valid. If this method returns an error, the activity will be
    /// discarded. This is separate from receive(), so that it can be called recursively on nested
    /// objects, without storing something in the database by accident.
    async fn verify(
        &self,
        data: &Data<Self::DataType>,
        request_counter: &mut i32,
    ) -> Result<(), Self::Error>;

    /// Receives the activity and stores its action in database.
    async fn receive(
        self,
        data: &Data<Self::DataType>,
        request_counter: &mut i32,
    ) -> Result<(), Self::Error>;
}

/// Allow for boxing of enum variants
#[async_trait::async_trait(?Send)]
impl<T> ActivityHandler for Box<T>
where
    T: ActivityHandler,
{
    type DataType = T::DataType;
    type Error = T::Error;

    fn id(&self) -> &Url {
        self.deref().id()
    }

    fn actor(&self) -> &Url {
        self.deref().actor()
    }

    async fn verify(
        &self,
        data: &Data<Self::DataType>,
        request_counter: &mut i32,
    ) -> Result<(), Self::Error> {
        self.verify(data, request_counter).await
    }

    async fn receive(
        self,
        data: &Data<Self::DataType>,
        request_counter: &mut i32,
    ) -> Result<(), Self::Error> {
        self.receive(data, request_counter).await
    }
}

#[async_trait::async_trait(?Send)]
pub trait ApubObject {
    type DataType;
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
        data: &Self::DataType,
    ) -> Result<Option<Self>, Self::Error>
    where
        Self: Sized;

    /// Marks the object as deleted in local db. Called when a delete activity is received, or if
    /// fetch returns a tombstone.
    async fn delete(self, _data: &Self::DataType) -> Result<(), Self::Error>
    where
        Self: Sized,
    {
        Ok(())
    }

    /// Trait for converting an object or actor into the respective ActivityPub type.
    async fn into_apub(self, data: &Self::DataType) -> Result<Self::ApubType, Self::Error>;

    /// Verify that the object is valid. If this method returns an error, it will be
    /// discarded. This is separate from from_apub(), so that it can be called recursively on nested
    /// objects, without storing something in the database by accident.
    async fn verify(
        apub: &Self::ApubType,
        expected_domain: &Url,
        data: &Self::DataType,
        request_counter: &mut i32,
    ) -> Result<(), Self::Error>;

    /// Converts an object from ActivityPub type to Lemmy internal type.
    ///
    /// * `apub` The object to read from
    /// * `context` LemmyContext which holds DB pool, HTTP client etc
    /// * `expected_domain` Domain where the object was received from. None in case of mod action.
    /// * `mod_action_allowed` True if the object can be a mod activity, ignore `expected_domain` in this case
    async fn from_apub(
        apub: Self::ApubType,
        data: &Self::DataType,
        request_counter: &mut i32,
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
