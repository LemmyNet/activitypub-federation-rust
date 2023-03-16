## Fetching data

After setting up our structs, implementing traits and initializing configuration, we can easily fetch data from remote servers:

```no_run
# use activitypub_federation::fetch::object_id::ObjectId;
# use activitypub_federation::traits::tests::DbUser;
# use activitypub_federation::config::FederationConfig;
# let db_connection = activitypub_federation::traits::tests::DbConnection;
# let _ = actix_rt::System::new();
# actix_rt::Runtime::new().unwrap().block_on(async {
let config = FederationConfig::builder()
    .domain("example.com")
    .app_data(db_connection)
    .build()?;
let user_id = ObjectId::<DbUser>::parse("https://mastodon.social/@LemmyDev")?;
let data = config.to_request_data();
let user = user_id.dereference(&data).await;
assert!(user.is_ok());
# Ok::<(), anyhow::Error>(())
}).unwrap()
```

`dereference` retrieves the object JSON at the given URL, and uses serde to convert it to `Person`. It then calls your method `ApubObject::from_apub` which inserts it in the database and returns a `DbUser` struct. `request_data` contains the federation config as well as a counter of outgoing HTTP requests. If this counter exceeds the configured maximum, further requests are aborted in order to avoid recursive fetching which could allow for a denial of service attack.

After dereferencing a remote object, it is stored in the local database and can be retrieved using [ObjectId::dereference_local](crate::fetch::object_id::ObjectId::dereference_local) without any network requests. This is important for performance reasons and for searching.

We can similarly dereference a user over webfinger with the following method. It fetches the webfinger response from `.well-known/webfinger` and then fetches the actor using [ObjectId::dereference](crate::fetch::object_id::ObjectId::dereference) as above.
```rust
# use activitypub_federation::traits::tests::DbConnection;
# use activitypub_federation::config::FederationConfig;
# use activitypub_federation::fetch::webfinger::webfinger_resolve_actor;
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

Note that webfinger queries don't contain a leading `@`. It is possible tha there are multiple Activitypub IDs returned for a single webfinger query in case of multiple actors with the same name (for example Lemmy permits group and person with the same name). In this case `webfinger_resolve_actor` automatically loops and returns the first item which can be dereferenced successfully to the given type.