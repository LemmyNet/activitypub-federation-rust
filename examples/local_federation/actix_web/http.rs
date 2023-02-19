use crate::{
    error::Error,
    instance::DatabaseHandle,
    objects::person::{DbUser, PersonAcceptedActivities},
};
use activitypub_federation::{
    config::FederationConfig,
    core::actix_web::inbox::receive_activity,
    protocol::context::WithContext,
    request_data::{ApubMiddleware, RequestData},
    traits::ApubObject,
    APUB_JSON_CONTENT_TYPE,
};
use actix_web::{web, web::Bytes, App, HttpRequest, HttpResponse, HttpServer};
use anyhow::anyhow;

pub fn listen(data: &FederationConfig<DatabaseHandle>) -> Result<(), Error> {
    let hostname = data.hostname();
    let data = data.clone();
    let server = HttpServer::new(move || {
        App::new()
            .wrap(ApubMiddleware::new(data.clone()))
            .route("/{user}", web::get().to(http_get_user))
            .route("/{user}/inbox", web::post().to(http_post_user_inbox))
    })
    .bind(hostname)?
    .run();
    actix_rt::spawn(server);
    Ok(())
}

/// Handles requests to fetch user json over HTTP
pub async fn http_get_user(
    user_name: web::Path<String>,
    data: RequestData<DatabaseHandle>,
) -> Result<HttpResponse, Error> {
    let db_user = data.local_user();
    if user_name.into_inner() == db_user.name {
        let apub_user = db_user.into_apub(&data).await?;
        Ok(HttpResponse::Ok()
            .content_type(APUB_JSON_CONTENT_TYPE)
            .json(WithContext::new_default(apub_user)))
    } else {
        Err(anyhow!("Invalid user").into())
    }
}

/// Handles messages received in user inbox
pub async fn http_post_user_inbox(
    request: HttpRequest,
    body: Bytes,
    data: RequestData<DatabaseHandle>,
) -> Result<HttpResponse, Error> {
    receive_activity::<WithContext<PersonAcceptedActivities>, DbUser, DatabaseHandle>(
        request, body, &data,
    )
    .await
}
