## Configuration

Next we need to do some configuration. Most importantly we need to specify the domain where the federated instance is running. It should be at the domain root and available over HTTPS for production. See the documentation for a list of config options. The parameter `user_data` is for anything that your application requires in handler functions, such as database connection handle, configuration etc.

```
# use activitypub_federation::config::FederationConfig;
# let db_connection = ();
# tokio::runtime::Runtime::new().unwrap().block_on(async {
let config = FederationConfig::builder()
    .domain("example.com")
    .app_data(db_connection)
    .build().await?;
# Ok::<(), anyhow::Error>(())
# }).unwrap()
```

`debug` is necessary to test federation with http and localhost URLs, but it should never be used in production. `url_verifier` can be used to implement a domain blacklist.
