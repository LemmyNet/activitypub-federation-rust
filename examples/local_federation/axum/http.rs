use crate::{
    error::Error,
    instance::DatabaseHandle,
    objects::person::{DbUser, Person, PersonAcceptedActivities},
};
use activitypub_federation::{
    config::FederationConfig,
    core::axum::{inbox::receive_activity, json::ApubJson, ActivityData},
    protocol::context::WithContext,
    request_data::{ApubMiddleware, RequestData},
    traits::ApubObject,
};
use anyhow::anyhow;
use axum::{
    extract::Path,
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use std::net::ToSocketAddrs;

pub fn listen(data: &FederationConfig<DatabaseHandle>) -> Result<(), Error> {
    let hostname = data.hostname();
    let data = data.clone();
    let app = Router::new()
        .route("/:user/inbox", post(http_post_user_inbox))
        .route("/:user", get(http_get_user))
        .layer(ApubMiddleware::new(data));

    let addr = hostname
        .to_socket_addrs()?
        .next()
        .expect("Failed to lookup domain name");
    let server = axum::Server::bind(&addr).serve(app.into_make_service());

    actix_rt::spawn(server);
    Ok(())
}

#[axum_macros::debug_handler]
async fn http_get_user(
    Path(user): Path<String>,
    data: RequestData<DatabaseHandle>,
) -> Result<ApubJson<WithContext<Person>>, Error> {
    let db_user = data.local_user();
    if user == db_user.name {
        let apub_user = db_user.into_apub(&data).await?;
        Ok(ApubJson(WithContext::new_default(apub_user)))
    } else {
        Err(anyhow!("Invalid user {user}").into())
    }
}

#[axum_macros::debug_handler]
async fn http_post_user_inbox(
    data: RequestData<DatabaseHandle>,
    activity_data: ActivityData,
) -> impl IntoResponse {
    receive_activity::<WithContext<PersonAcceptedActivities>, DbUser, DatabaseHandle>(
        activity_data,
        &data,
    )
    .await
}
