## Federating users

This library intentionally doesn't include any predefined data structures for federated data. The reason is that each federated application is different, and needs different data formats. Activitypub also doesn't define any specific data structures, but provides a few mandatory fields and many which are optional. For this reason it works best to let each application define its own data structures, and take advantage of serde for (de)serialization. This means we don't use `json-ld` which Activitypub is based on, but that doesn't cause any problems in practice.

The first thing we need to federate are users. Its easiest to get started by looking at the data sent by other platforms. Here we fetch an account from Mastodon, ignoring the many optional fields. This curl command is generally very helpful to inspect and debug federated services.

```text
$ curl -H 'Accept: application/activity+json' https://mastodon.social/@LemmyDev | jq
{
    "id": "https://mastodon.social/users/LemmyDev",
    "type": "Person",
    "preferredUsername": "LemmyDev",
    "name": "Lemmy",
    "inbox": "https://mastodon.social/users/LemmyDev/inbox",
    "outbox": "https://mastodon.social/users/LemmyDev/outbox",
    "publicKey": {
        "id": "https://mastodon.social/users/LemmyDev#main-key",
        "owner": "https://mastodon.social/users/LemmyDev",
        "publicKeyPem": "..."
    },
    ...
}
```

The most important fields are:
- `id`: Unique identifier for this object. At the same time it is the URL where we can fetch the object from
- `type`: The type of this object
- `preferredUsername`: Immutable username which was chosen at signup and is used in URLs as well as in mentions like `@LemmyDev@mastodon.social`
- `name`: Displayname which can be freely changed at any time
- `inbox`: URL where incoming activities are delivered to, treated in a later section
  see xx document for a definition of each field
- `publicKey`: Key which is used for [HTTP Signatures](https://datatracker.ietf.org/doc/html/draft-ietf-httpbis-message-signatures)

Refer to [Activity Vocabulary](https://www.w3.org/TR/activitystreams-vocabulary/) for further details and description of other fields. You can also inspect many other URLs on federated platforms with the given curl command.

Based on this we can define the following minimal struct to (de)serialize a `Person` with serde.

```rust
# use activitypub_federation::protocol::public_key::PublicKey;
# use activitypub_federation::fetch::object_id::ObjectId;
# use serde::{Deserialize, Serialize};
# use activitystreams_kinds::actor::PersonType;
# use activitypub_federation::url::Url;
# use activitypub_federation::traits::tests::DbUser;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    id: ObjectId<DbUser>,
    #[serde(rename = "type")]
    kind: PersonType,
    preferred_username: String,
    name: String,
    inbox: Url,
    outbox: Url,
    public_key: PublicKey,
}
```

`ObjectId` is a wrapper for `Url` which helps to fetch data from a remote server, and convert it to `DbUser` which is the type that's stored in our local database. It also helps with caching data so that it doesn't have to be refetched every time.

`PersonType` is an enum with a single variant `Person`. It is used to deserialize objects in a typesafe way: If the JSON type value does not match the string `Person`, deserialization fails. This helps in places where we don't know the exact data type that is being deserialized, as you will see later.

Besides we also need a second struct to represent the data which gets stored in our local database (for example PostgreSQL). This is necessary because the data format used by SQL is very different from that used by that from Activitypub. It is organized by an integer primary key instead of a link id. Nested structs are complicated to represent and easier if flattened. Some fields like `type` don't need to be stored at all. On the other hand, the database contains fields which can't be federated, such as the private key and a boolean indicating if the item is local or remote.

```rust
# use activitypub_federation::url::Url;
# use chrono::{DateTime, Utc};

pub struct DbUser {
    pub id: i32,
    pub name: String,
    pub display_name: String,
    pub password_hash: Option<String>,
    pub email: Option<String>,
    pub federation_id: Url,
    pub inbox: Url,
    pub outbox: Url,
    pub local: bool,
    pub public_key: String,
    pub private_key: Option<String>,
    pub last_refreshed_at: DateTime<Utc>,
}
```

Field names and other details of this type can be chosen freely according to your requirements. It only matters that the required data is being stored. Its important that this struct doesn't represent only local users who registered directly on our website, but also remote users that are registered on other instances and federated to us. The `local` column helps to easily distinguish both. It can also be distinguished from the domain of the `federation_id` URL, but that would be a much more expensive operation. All users have a `public_key`, but only local users have a `private_key`. On the other hand, `password_hash` and `email` are only present for local users. inbox` and `outbox` URLs need to be stored because each implementation is free to choose its own format for them, so they can't be regenerated on the fly.

In larger projects it makes sense to split this data in two. One for data relevant to local users (`password_hash`, `email` etc.) and one for data that is shared by both local and federated users (`federation_id`, `public_key` etc).

Finally we need to implement the traits [Object](crate::traits::Object) and [Actor](crate::traits::Actor) for `DbUser`. These traits are used to convert between `Person` and `DbUser` types. [Object::from_json](crate::traits::Object::from_json) must store the received object in database, so that it can later be retrieved without network calls using [Object::read_from_id](crate::traits::Object::read_from_id). Refer to the documentation for more details.
