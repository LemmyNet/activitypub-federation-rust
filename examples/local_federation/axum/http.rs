use crate::{
    error::Error,
    instance::DatabaseHandle,
    objects::person::{DbUser, Person, PersonAcceptedActivities},
};
use activitypub_federation::{
    axum::{
        inbox::{receive_activity, ActivityData},
        json::FederationJson,
    },
    config::{Data, FederationConfig, FederationMiddleware},
    fetch::webfinger::{build_webfinger_response, extract_webfinger_name, Webfinger},
    protocol::context::WithContext,
    traits::Object,
};
use axum::{
    extract::{Path, Query},
    response::IntoResponse,
    routing::{get, post},
    Json,
    Router,
};
use axum_macros::debug_handler;
use serde::Deserialize;
use std::net::ToSocketAddrs;
use tracing::info;

pub fn listen(config: &FederationConfig<DatabaseHandle>) -> Result<(), Error> {
    let hostname = config.domain();
    info!("Listening with axum on {hostname}");
    let config = config.clone();
    let app = Router::new()
        .route("/:user/inbox", post(http_post_user_inbox))
        .route("/:user", get(http_get_user))
        .route("/.well-known/webfinger", get(webfinger))
        .layer(FederationMiddleware::new(config));

    let addr = hostname
        .to_socket_addrs()?
        .next()
        .expect("Failed to lookup domain name");
    let server = axum::Server::bind(&addr).serve(app.into_make_service());

    actix_rt::spawn(server);
    Ok(())
}

#[debug_handler]
async fn http_get_user(
    Path(name): Path<String>,
    data: Data<DatabaseHandle>,
) -> Result<FederationJson<WithContext<Person>>, Error> {
    let db_user = data.read_user(&name)?;
    let json_user = db_user.into_json(&data).await?;
    Ok(FederationJson(WithContext::new_default(json_user)))
}

#[debug_handler]
async fn http_post_user_inbox(
    data: Data<DatabaseHandle>,
    activity_data: ActivityData,
) -> impl IntoResponse {
    receive_activity::<WithContext<PersonAcceptedActivities>, DbUser, DatabaseHandle>(
        activity_data,
        &data,
    )
    .await
}

#[derive(Deserialize)]
struct WebfingerQuery {
    resource: String,
}

#[debug_handler]
async fn webfinger(
    Query(query): Query<WebfingerQuery>,
    data: Data<DatabaseHandle>,
) -> Result<Json<Webfinger>, Error> {
    let name = extract_webfinger_name(&query.resource, &data)?;
    let db_user = data.read_user(&name)?;
    Ok(Json(build_webfinger_response(
        query.resource,
        db_user.ap_id.into_inner(),
    )))
}
