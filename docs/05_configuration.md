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

## IP validation and SSRF protection

To prevent SSRF attacks the library rejects URLs whose hostname resolves to an internal IP: via [`FederationConfig::is_valid_ip`](activitypub_federation::config::FederationConfig::is_valid_ip) for URLs with attacker-controlled hostnames, and via the DNS resolver at the moment of connecting (prevents DNS rebinding). Literal internal IPs never trigger DNS resolution and are only caught by `is_valid_ip`.

Configure the resolver with a host allowlist so internal services remain reachable in production. Allowlisted hosts may only be accessed on the given port, the DNS resolver itself has no access to ports and matches allowlisted hosts by name only. Attach it to your HTTP client, which also needs redirects disabled and timeouts set:

```
# use activitypub_federation::resolver::ValidatingDnsResolver;
# use reqwest::{redirect::Policy, Client};
# use std::time::Duration;
# use url::Url;
# let timeout = Duration::from_secs(10);
# (|| -> anyhow::Result<()> {
let allow_hosts = vec![
    Url::parse("https://example.com")?,   // local domain
    Url::parse("http://localhost:8080")?, // internal service
];
Client::builder()
   .redirect(Policy::none())
   .timeout(timeout)
   .connect_timeout(timeout)
   .dns_resolver(ValidatingDnsResolver::new(
       allow_hosts,
       !cfg!(debug_assertions),
   ))
#   .build()?;
# Ok::<(), anyhow::Error>(())
# })().unwrap();
```

`debug` mode (and the `DANGER_FEDERATION_ALLOW_LOCAL_IP` env var) skips IP validation entirely, for federation testing with local instances.
