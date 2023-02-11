use crate::{
    error::Error,
    instance::DatabaseHandle,
    objects::person::{MyUser, PersonAcceptedActivities},
};
use activitypub_federation::{
    core::{actix_web::inbox::receive_activity, object_id::ObjectId},
    deser::context::WithContext,
    request_data::{ApubContext, ApubMiddleware, RequestData},
    traits::ApubObject,
    APUB_JSON_CONTENT_TYPE,
};
use actix_web::{web, App, HttpRequest, HttpResponse, HttpServer};
use tokio::task;
use url::Url;

pub fn listen(data: &ApubContext<DatabaseHandle>) -> Result<(), Error> {
    let hostname = data.local_instance().hostname();
    let data = data.clone();
    let server = HttpServer::new(move || {
        App::new()
            .wrap(ApubMiddleware::new(data.clone()))
            .route("/objects/{user_name}", web::get().to(http_get_user))
            .service(
                web::scope("")
                    // Just a single, global inbox for simplicity
                    .route("/inbox", web::post().to(http_post_user_inbox)),
            )
    })
    .bind(hostname)?
    .run();
    task::spawn(server);
    Ok(())
}

/// Handles requests to fetch user json over HTTP
pub async fn http_get_user(
    request: HttpRequest,
    data: RequestData<DatabaseHandle>,
) -> Result<HttpResponse, Error> {
    let hostname: String = data.local_instance().hostname().to_string();
    let request_url = format!("http://{}{}", hostname, &request.uri().to_string());
    let url = Url::parse(&request_url)?;
    let user = ObjectId::<MyUser>::new(url)
        .dereference_local(&data)
        .await?
        .into_apub(&data)
        .await?;

    Ok(HttpResponse::Ok()
        .content_type(APUB_JSON_CONTENT_TYPE)
        .json(WithContext::new_default(user)))
}

/// Handles messages received in user inbox
pub async fn http_post_user_inbox(
    request: HttpRequest,
    payload: String,
    data: RequestData<DatabaseHandle>,
) -> Result<HttpResponse, Error> {
    let activity = serde_json::from_str(&payload)?;
    receive_activity::<WithContext<PersonAcceptedActivities>, MyUser, DatabaseHandle>(
        request, activity, &data,
    )
    .await
}
