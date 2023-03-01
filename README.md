Activitypub-Federation
===
[![Build Status](https://drone.join-lemmy.org/api/badges/LemmyNet/activitypub-federation-rust/status.svg)](https://drone.join-lemmy.org/LemmyNet/activitypub-federation-rust)
[![Crates.io](https://img.shields.io/crates/v/activitypub-federation.svg)](https://crates.io/crates/activitypub-federation)

A high-level framework for [ActivityPub](https://www.w3.org/TR/activitypub/) federation in Rust. The goal is to encapsulate all basic functionality, so that developers can easily use the protocol without any prior knowledge.

The ActivityPub protocol is a decentralized social networking protocol. It allows web servers to exchange data using JSON over HTTP. Data can be fetched on demand, and also delivered directly to inboxes for live updates.

While Activitypub is not in widespread use yet, is has the potential to form the basis of the next generation of social media. This is because it has a number of major advantages compared to existing platforms and alternative technologies:

- **Interoperability**: Imagine being able to comment under a Youtube video directly from twitter.com, and having the comment shown under the video on youtube.com. Or following a Subreddit from Facebook. Such functionality is already available on the equivalent Fediverse platforms, thanks to common usage of Activitypub.
- **Ease of use**: From a user perspective, decentralized social media works almost identically to existing websites: a website with email and password based login. Unlike pure peer-to-peer networks, it is not necessary to handle private keys or install any local software.
- **Open ecosystem**: All existing Fediverse software is open source, and there are no legal or bureaucratic requirements to start federating. That means anyone can create or fork federated software. In this way different software platforms can exist in the same network according to the preferences of different user groups. It is not necessary to target the lowest common denominator as with corporate social media.
- **Censorship resistance**: Current social media platforms are under the control of a few corporations and are actively being censored as revealed by the [Twitter Files](https://jordansather.substack.com/p/running-list-of-all-twitter-files). This would be much more difficult on a federated network, as it would require the cooperation of every single instance administrator. Additionally, users who are affected by censorship can create their own websites and stay connected with the network.
- **Low barrier to entry**: All it takes to host a federated website are a small server, a domain and a TLS certificate. All of this is easily in the reach of individual hobbyists. There is also some technical knowledge needed, but this can be avoided with managed hosting platforms.

Below is a complete guide that explains how to create a federated project from scratch.

Feel free to open an issue if you have any questions regarding this crate. You can also join the Matrix channel [#activitystreams](https://matrix.to/#/%23activitystreams:matrix.asonix.dog) for discussion about Activitypub in Rust. Additionally check out [Socialhub forum](https://socialhub.activitypub.rocks/) for general ActivityPub development.

## Overview

It is recommended to read the [W3C Activitypub standard document](https://www.w3.org/TR/activitypub/) which explains in detail how the protocol works. Note that it includes a section about client to server interactions, this functionality is not implemented by any major Fediverse project. Other relevant standard documents are [Activitystreams](https://www.w3.org/ns/activitystreams) and [Activity Vocabulary](https://www.w3.org/TR/activitystreams-vocabulary/). Its a good idea to keep these around as references during development.

This crate provides high level abstractions for the core functionality of Activitypub: fetching, sending and receiving data, as well as handling HTTP signatures. It was built from the experience of developing [Lemmy](https://join-lemmy.org/) which is the biggest Fediverse project written in Rust. Nevertheless it very generic and appropriate for any type of application wishing to implement the Activitypub protocol.

There are two examples included to see how the library altogether:

- `local_federation`: Creates two instances which run on localhost and federate with each other. This setup is ideal for quick development and well as automated tests.
- `live_federation`: A minimal application which can be deployed on a server and federate with other platforms such as Mastodon. For this it needs run at the root of a (sub)domain which is available over HTTPS. Edit `main.rs` to configure the server domain and your Fediverse handle. Once started, it will automatically send a message to you and log any incoming messages.

To see how this library is used in production, have a look at the [Lemmy federation code](https://github.com/LemmyNet/lemmy/tree/main/crates/apub).

## Setup

To use this crate in your project you need a web framework, preferably `actix-web` or `axum`. Be sure to enable the corresponding feature to access the full functionality. You also need a persistent storage such as PostgreSQL. Additionally, `serde` and `serde_json` are required to (de)serialize data which is sent over the network.

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

TODO: is outbox required by mastodon?

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
# use activitypub_federation::core::object_id::ObjectId;
# use serde::{Deserialize, Serialize};
# use activitystreams_kinds::actor::PersonType;
# use url::Url;
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

Besides we also need a second struct to represent the data which gets stored in our local database. This is necessary because the data format used by SQL is very different from that used by that from Activitypub. It is organized by an integer primary key instead of a link id. Nested structs are complicated to represent and easier if flattened. Some fields like `type` don't need to be stored at all. On the other hand, the database contains fields which can't be federated, such as the private key and a boolean indicating if the item is local or remote.

```rust
# use url::Url;

pub struct DbUser {
    pub id: i32,
    pub name: String,
    pub display_name: String,
    pub password_hash: Option<String>,
    pub email: Option<String>,
    pub apub_id: Url,
    pub inbox: Url,
    pub outbox: Url,
    pub local: bool,
    public_key: String,
    private_key: Option<String>,
}
```

Field names and other details of this type can be chosen freely according to your requirements. It only matters that the required data is being stored. Its important that this struct doesn't represent only local users who registered directly on our website, but also remote users that are registered on other instances and federated to us. The `local` column helps to easily distinguish both. It can also be distinguished from the domain of the `apub_id` URL, but that would be a much more expensive operation. All users have a `public_key`, but only local users have a `private_key`. On the other hand, `password_hash` and `email` are only present for local users. inbox` and `outbox` URLs need to be stored because each implementation is free to choose its own format for them, so they can't be regenerated on the fly.

In larger projects it makes sense to split this data in two. One for data relevant to local users (`password_hash`, `email` etc) and one for data that is shared by both local and federated users (`apub_id`, `public_key` etc).

Finally we need to implement the traits [ApubObject](crate::traits::ApubObject) and [Actor](crate::traits::Actor) for `DbUser`. These traits are used to convert between `Person` and `DbUser` types. [ApubObject::from_apub](crate::traits::ApubObject::from_apub) must store the received object in database, so that it can later be retrieved without network calls using [ApubObject::read_from_apub_id](crate::traits::ApubObject::read_from_apub_id). Refer to the documentation for more details.

## Configuration and fetching data

Next we need to do some configuration. Most importantly we need to specify the domain where the federated instance is running. It should be at the domain root and available over HTTPS for production. See the documentation for a list of config options. The parameter `user_data` is for anything that your application requires in handler functions, such as database connection handle, configuration etc.

```
# use activitypub_federation::config::FederationConfig;
# let db_connection = ();
# let _ = actix_rt::System::new();
let config = FederationConfig::builder()
    .domain("example.com")
    .app_data(db_connection)
    .build()?;
# Ok::<(), anyhow::Error>(())
```

With this we can already fetch data from remote servers:

```no_run
# use activitypub_federation::core::object_id::ObjectId;
# use activitypub_federation::traits::tests::DbUser;
# use activitypub_federation::config::FederationConfig;
# let db_connection = activitypub_federation::traits::tests::DbConnection;
# let _ = actix_rt::System::new();
# actix_rt::Runtime::new().unwrap().block_on(async {
let config = FederationConfig::builder().domain("example.com").app_data(db_connection).build()?;
let user_id = ObjectId::<DbUser>::new("https://mastodon.social/@LemmyDev")?;
let data = config.to_request_data();
let user = user_id.dereference(&data).await;
assert!(user.is_ok());
# Ok::<(), anyhow::Error>(())
}).unwrap()
```

`dereference` retrieves the object JSON at the given URL, and uses serde to convert it to `Person`. It then calls your method `ApubObject::from_apub` which inserts it in the database and returns a `DbUser` struct. `request_data` contains the federation config as well as a counter of outgoing HTTP requests. If this counter exceeds the configured maximum, further requests are aborted in order to avoid recursive fetching which could allow for a denial of service attack.

After dereferencing a remote object, it is stored in the local database and can be retrieved using [ObjectId::dereference_local](crate::core::object_id::ObjectId::dereference_local) without any network requests. This is important for performance reasons and for searching.

## Federating posts

We repeat the same steps taken above for users in order to federate our posts.

```text
$ curl -H 'Accept: application/activity+json' https://mastodon.social/@LemmyDev/109790106847504642 | jq
{
    "id": "https://mastodon.social/users/LemmyDev/statuses/109790106847504642",
    "type": "Note",
    "content": "<p><a href=\"https://mastodon.social/tags/lemmy\" ...",
    "attributedTo": "https://mastodon.social/users/LemmyDev",
    "to": [
        "https://www.w3.org/ns/activitystreams#Public"
    ],
    "cc": [
        "https://mastodon.social/users/LemmyDev/followers"
    ],
}
```

The most important fields are:
- `id`: Unique identifier for this object. At the same time it is the URL where we can fetch the object from
- `type`: The type of this object
- `content`: Post text in HTML format
- `attributedTo`: ID of the user who created this post
- `to`, `cc`: Who the object is for. The special "public" URL indicates that everyone can view it.  It also gets delivered to followers of the LemmyDev account.

Just like for `Person` before, we need to implement a protocol type and a database type, then implement trait `ApubObject`. See the example for details.

## HTTP endpoints

The next step is to allow other servers to fetch our actors and objects. For this we need to create an HTTP route, most commonly at the same path where the actor or object can be viewed in a web browser. On this path there should be another route which responds to requests with header `Accept: application/activity+json` and serves the JSON data. This needs to be done for all actors and objects. Note that only local items should be served in this way.

```no_run
# use std::net::SocketAddr;
# use activitypub_federation::config::FederationConfig;
# use activitypub_federation::protocol::context::WithContext;
# use activitypub_federation::core::axum::json::ApubJson;
# use anyhow::Error;
# use activitypub_federation::traits::tests::Person;
# use activitypub_federation::config::RequestData;
# use activitypub_federation::traits::tests::DbConnection;
# use axum::extract::Path;
# use activitypub_federation::config::ApubMiddleware;
# use axum::routing::get;
# use crate::activitypub_federation::traits::ApubObject;
# use axum::headers::ContentType;
# use activitypub_federation::APUB_JSON_CONTENT_TYPE;
# use axum::TypedHeader;
# use axum::response::IntoResponse;
# use http::HeaderMap;
# async fn generate_user_html(_: String, _: RequestData<DbConnection>) -> axum::response::Response { todo!() }

#[actix_rt::main]
async fn main() -> Result<(), Error> {
    let data = FederationConfig::builder()
        .domain("example.com")
        .app_data(DbConnection)
        .build()?;
        
    let app = axum::Router::new()
        .route("/user/:name", get(http_get_user))
        .layer(ApubMiddleware::new(data));

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    tracing::debug!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await?;
    Ok(())
}

async fn http_get_user(
    header_map: HeaderMap,
    Path(name): Path<String>,
    data: RequestData<DbConnection>,
) -> impl IntoResponse {
    let accept = header_map.get("accept").map(|v| v.to_str().unwrap());
    if accept == Some(APUB_JSON_CONTENT_TYPE) {
        let db_user = data.read_local_user(name).await.unwrap();
        let apub_user = db_user.into_apub(&data).await.unwrap();
        ApubJson(WithContext::new_default(apub_user)).into_response()
    }
    else {
        generate_user_html(name, data).await
    }
}
```

There are a couple of things going on here. Like before we are constructing the federation config with our domain and application data. We pass this to a middleware to make it available in request handlers, then listening on a port with the axum webserver.

The `http_get_user` method allows retrieving a user profile from `/user/:name`. It checks the `accept` header, and compares it to the one used by Activitypub (`application/activity+json`). If it matches, the user is read from database and converted to Activitypub json format. The `context` field is added (`WithContext` for `json-ld` compliance), and it is converted to a JSON response with header `content-type: application/activity+json` using `ApubJson`. It can now be retrieved with the command `curl -H 'Accept: application/activity+json' ...` introduced earlier, or with `ObjectId`.

If the `accept` header doesn't match, it renders the user profile as HTML for viewing in a web browser.

## Webfinger

Webfinger can resolve a handle like `@nutomic@lemmy.ml` into an ID like `https://lemmy.ml/u/nutomic` which can be used by Activitypub. Webfinger is not part of the ActivityPub standard, but the fact that Mastodon requires it makes it de-facto mandatory. It is defined in [RFC 7033](https://www.rfc-editor.org/rfc/rfc7033). Implementing it basically means handling requests of the form`https://mastodon.social/.well-known/webfinger?resource=acct:LemmyDev@mastodon.social`.

To do this we can implement the following HTTP handler which must be bound to path `.well-known/webfinger`.

```rust
# use serde::Deserialize;
# use axum::{extract::Query, Json};
# use activitypub_federation::config::RequestData;
# use activitypub_federation::webfinger::Webfinger;
# use anyhow::Error;
# use activitypub_federation::traits::tests::DbConnection;
# use activitypub_federation::webfinger::extract_webfinger_name;
# use activitypub_federation::webfinger::build_webfinger_response;

#[derive(Deserialize)]
struct WebfingerQuery {
    resource: String,
}

async fn webfinger(
    Query(query): Query<WebfingerQuery>,
    data: RequestData<DbConnection>,
) -> Result<Json<Webfinger>, Error> {
    let name = extract_webfinger_name(&query.resource, &data)?;
    let db_user = data.read_local_user(name).await?;
    Ok(Json(build_webfinger_response(query.resource, db_user.apub_id)))
}
```

The resolve a user via webfinger call the following method:
```rust
# use activitypub_federation::traits::tests::DbConnection;
# use activitypub_federation::config::FederationConfig;
# use activitypub_federation::webfinger::webfinger_resolve_actor;
# use activitypub_federation::traits::tests::DbUser;
# let db_connection = DbConnection;
# let _ = actix_rt::System::new();
# actix_rt::Runtime::new().unwrap().block_on(async {
# let config = FederationConfig::builder().domain("example.com").app_data(db_connection).build()?;
# let data = config.to_request_data();
let user: DbUser = webfinger_resolve_actor("nutomic@lemmy.ml", &data).await?;
# Ok::<(), anyhow::Error>(())
# }).unwrap();
```

Note that webfinger queries don't contain a leading `@`. `webfinger_resolve_actor` fetches the webfinger response, finds the matching actor ID, fetches the actor using [ObjectId::dereference](crate::core::object_id::ObjectId::dereference) and converts it with [ApubObject::from_apub](crate::traits::ApubObject::from_apub). It is possible tha there are multiple Activitypub IDs returned for a single webfinger query in case of multiple actors with the same name (for example Lemmy permits group and person with the same name). In this case `webfinger_resolve_actor` automatically loops and returns the first item which can be dereferenced successfully to the given type.

## Sending and receiving activities

TODO: continue here

```text
#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Follow {
    pub actor: ObjectId<DbUser>,
    pub object: ObjectId<DbUser>,
    #[serde(rename = "type")]
    pub kind: FollowType,
    pub id: Url,
}
```

## Fetching remote object with unknown type

It is sometimes necessary to fetch from a URL, but we don't know the exact type of object it will return. An example is the search field in most federated platforms, which allows pasting and `id` URL and fetches it from the origin server. It can implemented in the following way:

```no_run
# use activitypub_federation::traits::tests::{DbUser, DbPost};
# use activitypub_federation::core::object_id::ObjectId;
# use activitypub_federation::traits::ApubObject;
# use activitypub_federation::config::FederationConfig;
# use serde::{Deserialize, Serialize};
# use activitypub_federation::traits::tests::DbConnection;
# use activitypub_federation::config::RequestData;
# use url::Url;
# use activitypub_federation::traits::tests::{Person, Note};

pub enum SearchableDbObjects {
    User(DbUser),
    Post(DbPost)
}

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
pub enum SearchableApubObjects {
    Person(Person),
    Note(Note)
}

#[async_trait::async_trait]
impl ApubObject for SearchableDbObjects {
    type DataType = DbConnection;
    type ApubType = SearchableApubObjects;
    type Error = anyhow::Error;
    
    async fn read_from_apub_id(
        object_id: Url,
        data: &RequestData<Self::DataType>,
    ) -> Result<Option<Self>, Self::Error> {
        Ok(None)
    }
    
    async fn into_apub(
        self,
        data: &RequestData<Self::DataType>,
    ) -> Result<Self::ApubType, Self::Error> {
        unimplemented!();
    }
    
    async fn from_apub(
        apub: Self::ApubType,
        data: &RequestData<Self::DataType>,
    ) -> Result<Self, Self::Error> {
        use SearchableDbObjects::*;
        match apub {
            SearchableApubObjects::Person(p) => Ok(User(DbUser::from_apub(p, data).await?)),
            SearchableApubObjects::Note(n) => Ok(Post(DbPost::from_apub(n, data).await?)),
        }
    }
}

#[actix_rt::main]
async fn main() -> Result<(), anyhow::Error> {
    # let config = FederationConfig::builder().domain("example.com").app_data(DbConnection).build().unwrap();
    # let data = config.to_request_data();
    let query = "https://example.com/id/413";
    let query_result = ObjectId::<SearchableDbObjects>::new(query)?
        .dereference(&data)
        .await?;
    match query_result {
        SearchableDbObjects::Post(post) => {} // retrieved object is a post
        SearchableDbObjects::User(user) => {} // object is a user
    };
    Ok(())
}
```

What this does is...

## License

Licensed under [AGPLv3](/LICENSE).
