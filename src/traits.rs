//! Traits which need to be implemented for federated data types

use crate::{config::RequestData, protocol::public_key::PublicKey};
use async_trait::async_trait;
use chrono::NaiveDateTime;
use std::ops::Deref;
use url::Url;

/// Helper for converting between database structs and federated protocol structs.
///
/// ```
/// # use chrono::{Local, NaiveDateTime};
/// # use url::Url;
/// # use activitypub_federation::protocol::public_key::PublicKey;
/// # use activitypub_federation::config::RequestData;
/// use activitypub_federation::protocol::verification::verify_domains_match;
/// # use activitypub_federation::traits::ApubObject;
/// # use activitypub_federation::traits::tests::{DbConnection, Person};
/// # pub struct DbUser {
/// #     pub name: String,
/// #     pub ap_id: Url,
/// #     pub inbox: Url,
/// #     pub public_key: String,
/// #     pub private_key: Option<String>,
/// #     pub local: bool,
/// #     pub last_refreshed_at: NaiveDateTime,
/// # }
///
/// #[async_trait::async_trait]
/// impl ApubObject for DbUser {
///     type DataType = DbConnection;
///     type ApubType = Person;
///     type Error = anyhow::Error;
///
/// fn last_refreshed_at(&self) -> Option<NaiveDateTime> {
///     Some(self.last_refreshed_at)
/// }
///
/// async fn read_from_apub_id(object_id: Url, data: &RequestData<Self::DataType>) -> Result<Option<Self>, Self::Error> {
///         // Attempt to read object from local database. Return Ok(None) if not found.
///         let user: Option<DbUser> = data.read_user_from_apub_id(object_id).await?;
///         Ok(user)
///     }
///
/// async fn into_apub(self, data: &RequestData<Self::DataType>) -> Result<Self::ApubType, Self::Error> {
///         // Called when a local object gets sent out over Activitypub. Simply convert it to the
///         // protocol struct
///         Ok(Person {
///             kind: Default::default(),
///             preferred_username: self.name,
///             id: self.ap_id.clone().into(),
///             inbox: self.inbox,
///             public_key: PublicKey::new(self.ap_id, self.public_key),
///         })
///     }
///
///     async fn verify(apub: &Self::ApubType, expected_domain: &Url, data: &RequestData<Self::DataType>,) -> Result<(), Self::Error> {
///         verify_domains_match(apub.id.inner(), expected_domain)?;
///         // additional application specific checks
///         Ok(())
///     }
///
///     async fn from_apub(apub: Self::ApubType, data: &RequestData<Self::DataType>) -> Result<Self, Self::Error> {
///         // Called when a remote object gets received over Activitypub. Validate and insert it
///         // into the database.
///
///         let user = DbUser {
///             name: apub.preferred_username,
///             ap_id: apub.id.into_inner(),
///             inbox: apub.inbox,
///             public_key: apub.public_key.public_key_pem,
///             private_key: None,
///             local: false,
///             last_refreshed_at: Local::now().naive_local(),
///         };
///
///         // Make sure not to overwrite any local object
///         if data.domain() == user.ap_id.domain().unwrap() {
///             // Activitypub doesnt distinguish between creating and updating an object. Thats why we
///             // need to use upsert functionality here
///             data.upsert(&user).await?;
///         }
///         Ok(user)
///     }
///
/// }
#[async_trait]
pub trait ApubObject: Sized {
    /// App data type passed to handlers. Must be identical to
    /// [crate::config::FederationConfigBuilder::app_data] type.
    type DataType: Clone + Send + Sync;
    /// The type of protocol struct which gets sent over network to federate this database struct.
    type ApubType;
    /// Error type returned by handler methods
    type Error;

    /// Returns the last time this object was updated.
    ///
    /// If this returns `Some` and the value is too long ago, the object is refetched from the
    /// original instance. This should always be implemented for actors, because there is no active
    /// update mechanism prescribed. It is possible to send `Update/Person` activities for profile
    /// changes, but not all implementations do this, so `last_refreshed_at` is still necessary.
    ///
    /// The object is refetched if `last_refreshed_at` value is more than 24 hours ago. In debug
    /// mode this is reduced to 20 seconds.
    fn last_refreshed_at(&self) -> Option<NaiveDateTime> {
        None
    }

    /// Try to read the object with given `id` from local database.
    ///
    /// Should return `Ok(None)` if not found.
    async fn read_from_apub_id(
        object_id: Url,
        data: &RequestData<Self::DataType>,
    ) -> Result<Option<Self>, Self::Error>;

    /// Mark remote object as deleted in local database.
    ///
    /// Called when a `Delete` activity is received, or if fetch returns a `Tombstone` object.
    async fn delete(self, _data: &RequestData<Self::DataType>) -> Result<(), Self::Error> {
        Ok(())
    }

    /// Convert database type to Activitypub type.
    ///
    /// Called when a local object gets fetched by another instance over HTTP, or when an object
    /// gets sent in an activity.
    async fn into_apub(
        self,
        data: &RequestData<Self::DataType>,
    ) -> Result<Self::ApubType, Self::Error>;

    /// Verifies that the received object is valid.
    ///
    /// You should check here that the domain of id matches `expected_domain`. Additionally you
    /// should perform any application specific checks.
    ///
    /// It is necessary to use a separate method for this, because it might be used for activities
    /// like `Delete/Note`, which shouldn't perform any database write for the inner `Note`.
    async fn verify(
        apub: &Self::ApubType,
        expected_domain: &Url,
        data: &RequestData<Self::DataType>,
    ) -> Result<(), Self::Error>;

    /// Convert object from ActivityPub type to database type.
    ///
    /// Called when an object is received from HTTP fetch or as part of an activity. This method
    /// should do verification and write the received object to database. Note that there is no
    /// distinction between create and update, so an `upsert` operation should be used.
    async fn from_apub(
        apub: Self::ApubType,
        data: &RequestData<Self::DataType>,
    ) -> Result<Self, Self::Error>;
}

/// Handler for receiving incoming activities.
///
/// ```
/// # use activitystreams_kinds::activity::FollowType;
/// # use url::Url;
/// # use activitypub_federation::fetch::object_id::ObjectId;
/// # use activitypub_federation::config::RequestData;
/// # use activitypub_federation::traits::ActivityHandler;
/// # use activitypub_federation::traits::tests::{DbConnection, DbUser};
/// #[derive(serde::Deserialize)]
/// struct Follow {
///     actor: ObjectId<DbUser>,
///     object: ObjectId<DbUser>,
///     #[serde(rename = "type")]
///     kind: FollowType,
///     id: Url,
/// }
///
/// #[async_trait::async_trait]
/// impl ActivityHandler for Follow {
///     type DataType = DbConnection;
///     type Error = anyhow::Error;
///
///     fn id(&self) -> &Url {
///         &self.id
///     }
///
///     fn actor(&self) -> &Url {
///         self.actor.inner()
///     }
///
///     async fn verify(&self, data: &RequestData<Self::DataType>) -> Result<(), Self::Error> {
///         Ok(())
///     }
///
///     async fn receive(self, data: &RequestData<Self::DataType>) -> Result<(), Self::Error> {
///         let local_user = self.object.dereference(data).await?;
///         let follower = self.actor.dereference(data).await?;
///         data.add_follower(local_user, follower).await?;
///         Ok(())
///     }
/// }
/// ```
#[async_trait]
#[enum_delegate::register]
pub trait ActivityHandler {
    /// App data type passed to handlers. Must be identical to
    /// [crate::config::FederationConfigBuilder::app_data] type.
    type DataType: Clone + Send + Sync;
    /// Error type returned by handler methods
    type Error;

    /// `id` field of the activity
    fn id(&self) -> &Url;

    /// `actor` field of activity
    fn actor(&self) -> &Url;

    /// Verifies that the received activity is valid.
    ///
    /// This needs to be a separate method, because it might be used for activities
    /// like `Undo/Follow`, which shouldn't perform any database write for the inner `Follow`.
    async fn verify(&self, data: &RequestData<Self::DataType>) -> Result<(), Self::Error>;

    /// Called when an activity is received.
    ///
    /// Should perform validation and possibly write action to the database. In case the activity
    /// has a nested `object` field, must call `object.from_apub` handler.
    async fn receive(self, data: &RequestData<Self::DataType>) -> Result<(), Self::Error>;
}

/// Trait to allow retrieving common Actor data.
pub trait Actor: ApubObject {
    /// `id` field of the actor
    fn id(&self) -> &Url;

    /// The actor's public key for verifying signatures of incoming activities.
    ///
    /// Use [generate_actor_keypair](crate::http_signatures::generate_actor_keypair) to create the
    /// actor keypair.
    fn public_key_pem(&self) -> &str;

    /// The inbox where activities for this user should be sent to
    fn inbox(&self) -> Url;

    /// Generates a public key struct for use in the actor json representation
    fn public_key(&self) -> PublicKey {
        PublicKey::new(self.id().clone(), self.public_key_pem().to_string())
    }

    /// The actor's shared inbox, if any
    fn shared_inbox(&self) -> Option<Url> {
        None
    }

    /// Returns shared inbox if it exists, normal inbox otherwise.
    fn shared_inbox_or_inbox(&self) -> Url {
        self.shared_inbox().unwrap_or_else(|| self.inbox())
    }
}

/// Allow for boxing of enum variants
#[async_trait]
impl<T> ActivityHandler for Box<T>
where
    T: ActivityHandler + Send + Sync,
{
    type DataType = T::DataType;
    type Error = T::Error;

    fn id(&self) -> &Url {
        self.deref().id()
    }

    fn actor(&self) -> &Url {
        self.deref().actor()
    }

    async fn verify(&self, data: &RequestData<Self::DataType>) -> Result<(), Self::Error> {
        (*self).verify(data).await
    }

    async fn receive(self, data: &RequestData<Self::DataType>) -> Result<(), Self::Error> {
        (*self).receive(data).await
    }
}

/// Some impls of these traits for use in tests. Dont use this from external crates.
///
/// TODO: Should be using `cfg[doctest]` but blocked by <https://github.com/rust-lang/rust/issues/67295>
#[doc(hidden)]
#[allow(clippy::unwrap_used)]
pub mod tests {
    use super::*;
    use crate::{
        fetch::object_id::ObjectId,
        http_signatures::{generate_actor_keypair, Keypair},
        protocol::{public_key::PublicKey, verification::verify_domains_match},
    };
    use activitystreams_kinds::{activity::FollowType, actor::PersonType};
    use anyhow::Error;
    use once_cell::sync::Lazy;
    use serde::{Deserialize, Serialize};

    #[derive(Clone)]
    pub struct DbConnection;

    impl DbConnection {
        pub async fn read_user_from_apub_id<T>(&self, _: Url) -> Result<Option<T>, Error> {
            Ok(None)
        }
        pub async fn read_local_user(&self, _: String) -> Result<DbUser, Error> {
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
        pub apub_id: Url,
        pub inbox: Url,
        pub public_key: String,
        #[allow(dead_code)]
        private_key: Option<String>,
        pub followers: Vec<Url>,
        pub local: bool,
    }

    pub static DB_USER_KEYPAIR: Lazy<Keypair> = Lazy::new(|| generate_actor_keypair().unwrap());

    pub static DB_USER: Lazy<DbUser> = Lazy::new(|| DbUser {
        name: String::new(),
        apub_id: "https://localhost/123".parse().unwrap(),
        inbox: "https://localhost/123/inbox".parse().unwrap(),
        public_key: DB_USER_KEYPAIR.public_key.clone(),
        private_key: None,
        followers: vec![],
        local: false,
    });

    #[async_trait]
    impl ApubObject for DbUser {
        type DataType = DbConnection;
        type ApubType = Person;
        type Error = Error;

        async fn read_from_apub_id(
            _object_id: Url,
            _data: &RequestData<Self::DataType>,
        ) -> Result<Option<Self>, Self::Error> {
            Ok(Some(DB_USER.clone()))
        }

        async fn into_apub(
            self,
            _data: &RequestData<Self::DataType>,
        ) -> Result<Self::ApubType, Self::Error> {
            let public_key = PublicKey::new(self.apub_id.clone(), self.public_key.clone());
            Ok(Person {
                preferred_username: self.name.clone(),
                kind: Default::default(),
                id: self.apub_id.into(),
                inbox: self.inbox,
                public_key,
            })
        }

        async fn verify(
            apub: &Self::ApubType,
            expected_domain: &Url,
            _data: &RequestData<Self::DataType>,
        ) -> Result<(), Self::Error> {
            verify_domains_match(apub.id.inner(), expected_domain)?;
            Ok(())
        }

        async fn from_apub(
            apub: Self::ApubType,
            _data: &RequestData<Self::DataType>,
        ) -> Result<Self, Self::Error> {
            Ok(DbUser {
                name: apub.preferred_username,
                apub_id: apub.id.into(),
                inbox: apub.inbox,
                public_key: apub.public_key.public_key_pem,
                private_key: None,
                followers: vec![],
                local: false,
            })
        }
    }

    impl Actor for DbUser {
        fn id(&self) -> &Url {
            &self.apub_id
        }

        fn public_key_pem(&self) -> &str {
            &self.public_key
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
    impl ActivityHandler for Follow {
        type DataType = DbConnection;
        type Error = Error;

        fn id(&self) -> &Url {
            &self.id
        }

        fn actor(&self) -> &Url {
            self.actor.inner()
        }

        async fn verify(&self, _: &RequestData<Self::DataType>) -> Result<(), Self::Error> {
            Ok(())
        }

        async fn receive(self, _data: &RequestData<Self::DataType>) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Note {}
    #[derive(Debug, Clone)]
    pub struct DbPost {}

    #[async_trait]
    impl ApubObject for DbPost {
        type DataType = DbConnection;
        type ApubType = Note;
        type Error = Error;

        async fn read_from_apub_id(
            _: Url,
            _: &RequestData<Self::DataType>,
        ) -> Result<Option<Self>, Self::Error> {
            todo!()
        }

        async fn into_apub(
            self,
            _: &RequestData<Self::DataType>,
        ) -> Result<Self::ApubType, Self::Error> {
            todo!()
        }

        async fn verify(
            _: &Self::ApubType,
            _: &Url,
            _: &RequestData<Self::DataType>,
        ) -> Result<(), Self::Error> {
            todo!()
        }

        async fn from_apub(
            _: Self::ApubType,
            _: &RequestData<Self::DataType>,
        ) -> Result<Self, Self::Error> {
            todo!()
        }
    }
}
