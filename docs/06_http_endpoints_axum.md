## HTTP endpoints

The next step is to allow other servers to fetch our actors and objects. For this we need to create an HTTP route, most commonly at the same path where the actor or object can be viewed in a web browser. On this path there should be another route which responds to requests with header `Accept: application/activity+json` and serves the JSON data. This needs to be done for all actors and objects. Note that only local items should be served in this way.

```no_run
# use std::net::SocketAddr;
# use activitypub_federation::config::FederationConfig;
# use activitypub_federation::protocol::context::WithContext;
# use activitypub_federation::axum::json::FederationJson;
# use anyhow::Error;
# use activitypub_federation::traits::tests::Person;
# use activitypub_federation::config::Data;
# use activitypub_federation::traits::tests::DbConnection;
# use axum::extract::Path;
# use activitypub_federation::config::FederationMiddleware;
# use axum::routing::get;
# use crate::activitypub_federation::traits::Object;
# use axum::headers::ContentType;
# use activitypub_federation::FEDERATION_CONTENT_TYPE;
# use axum::TypedHeader;
# use axum::response::IntoResponse;
# use http::HeaderMap;
# async fn generate_user_html(_: String, _: Data<DbConnection>) -> axum::response::Response { todo!() }

#[tokio::main]
async fn main() -> Result<(), Error> {
    let data = FederationConfig::builder()
        .domain("example.com")
        .app_data(DbConnection)
        .build().await?;
        
    let app = axum::Router::new()
        .route("/user/:name", get(http_get_user))
        .layer(FederationMiddleware::new(data));

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
    data: Data<DbConnection>,
) -> impl IntoResponse {
    let accept = header_map.get("accept").map(|v| v.to_str().unwrap());
    if accept == Some(FEDERATION_CONTENT_TYPE) {
        let db_user = data.read_local_user(name).await.unwrap();
        let json_user = db_user.into_json(&data).await.unwrap();
        FederationJson(WithContext::new_default(json_user)).into_response()
    }
    else {
        generate_user_html(name, data).await
    }
}
```

There are a couple of things going on here. Like before we are constructing the federation config with our domain and application data. We pass this to a middleware to make it available in request handlers, then listening on a port with the axum webserver.

The `http_get_user` method allows retrieving a user profile from `/user/:name`. It checks the `accept` header, and compares it to the one used by Activitypub (`application/activity+json`). If it matches, the user is read from database and converted to Activitypub json format. The `context` field is added (`WithContext` for `json-ld` compliance), and it is converted to a JSON response with header `content-type: application/activity+json` using `FederationJson`. It can now be retrieved with the command `curl -H 'Accept: application/activity+json' ...` introduced earlier, or with `ObjectId`.

If the `accept` header doesn't match, it renders the user profile as HTML for viewing in a web browser.

We also need to implement a webfinger endpoint, which can resolve a handle like `@nutomic@lemmy.ml` into an ID like `https://lemmy.ml/u/nutomic` that can be used by Activitypub. Webfinger is not part of the ActivityPub standard, but the fact that Mastodon requires it makes it de-facto mandatory. It is defined in [RFC 7033](https://www.rfc-editor.org/rfc/rfc7033). Implementing it basically means handling requests of the form`https://mastodon.social/.well-known/webfinger?resource=acct:LemmyDev@mastodon.social`.

To do this we can implement the following HTTP handler which must be bound to path `.well-known/webfinger`.

```rust
# use serde::Deserialize;
# use axum::{extract::Query, Json};
# use activitypub_federation::config::Data;
# use activitypub_federation::fetch::webfinger::Webfinger;
# use anyhow::Error;
# use activitypub_federation::traits::tests::DbConnection;
# use activitypub_federation::fetch::webfinger::extract_webfinger_name;
# use activitypub_federation::fetch::webfinger::build_webfinger_response;

#[derive(Deserialize)]
struct WebfingerQuery {
    resource: String,
}

async fn webfinger(
    Query(query): Query<WebfingerQuery>,
    data: Data<DbConnection>,
) -> Result<Json<Webfinger>, Error> {
    let name = extract_webfinger_name(&query.resource, &data)?;
    let db_user = data.read_local_user(name).await?;
    Ok(Json(build_webfinger_response(query.resource, db_user.federation_id)))
}
```
