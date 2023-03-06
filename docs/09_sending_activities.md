## Sending activities

To send an activity we need to initialize our previously defined struct, and pick an actor for sending. We also need a list of all actors that should receive the activity.

```
# use activitypub_federation::config::FederationConfig;
# use activitypub_federation::activity_queue::send_activity;
# use activitypub_federation::http_signatures::generate_actor_keypair;
# use activitypub_federation::traits::Actor;
# use activitypub_federation::fetch::object_id::ObjectId;
# use activitypub_federation::traits::tests::{DB_USER, DbConnection, Follow};
# let _ = actix_rt::System::new();
# actix_rt::Runtime::new().unwrap().block_on(async {
# let db_connection = DbConnection;
# let config = FederationConfig::builder()
#     .domain("example.com")
#     .app_data(db_connection)
#     .build()?;
# let data = config.to_request_data();
# let recipient = DB_USER.clone();
// Each actor has a keypair. Generate it on signup and store it in the database.
let keypair = generate_actor_keypair()?;
let activity = Follow {
    actor: ObjectId::new("https://lemmy.ml/u/nutomic")?,
    object: recipient.apub_id.clone().into(),
    kind: Default::default(),
    id: "https://lemmy.ml/activities/321".try_into()?
};
let inboxes = vec![recipient.shared_inbox_or_inbox()];
send_activity(activity, keypair.private_key, inboxes, &data).await?;
# Ok::<(), anyhow::Error>(())
# }).unwrap()
```

The list of inboxes gets deduplicated (important for shared inbox). All inboxes on the local
domain and those which fail the [crate::config::UrlVerifier] check are excluded from delivery.
For each remaining inbox a background tasks is created. It signs the HTTP header with the given
private key. Finally the activity is delivered to the inbox.

It is possible that delivery fails because the target instance is temporarily unreachable. In
this case the task is scheduled for retry after a certain waiting time. For each task delivery
is retried up to 3 times after the initial attempt. The retry intervals are as follows:
- one minute, in case of service restart
- one hour, in case of instance maintenance
- 2.5 days, in case of major incident with rebuild from backup

In case [crate::config::FederationConfigBuilder::debug] is enabled, no background thread is used but activities are sent directly on the foreground. This makes it easier to catch delivery errors and avoids complicated steps to await delivery in tests.
