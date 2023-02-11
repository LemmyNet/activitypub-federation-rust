use crate::{
    error::Error,
    instance::DatabaseHandle,
    objects::person::{MyUser, Person, PersonAcceptedActivities},
};
use activitypub_federation::{
    core::{
        axum::{inbox::receive_activity, json::ApubJson, verify_request_payload, DigestVerified},
        object_id::ObjectId,
    },
    deser::context::WithContext,
    request_data::{ApubContext, ApubMiddleware, RequestData},
    traits::ApubObject,
};
use axum::{
    body,
    extract::OriginalUri,
    middleware,
    response::IntoResponse,
    routing::{get, post},
    Extension,
    Json,
    Router,
};
use http::{HeaderMap, Method, Request};
use hyper::Body;
use std::net::ToSocketAddrs;
use tokio::task;
use tower::ServiceBuilder;
use tower_http::{trace::TraceLayer, ServiceBuilderExt};
use url::Url;

pub fn listen(data: &ApubContext<DatabaseHandle>) -> Result<(), Error> {
    let hostname = data.local_instance().hostname();
    let data = data.clone();
    let app = Router::new()
        .route("/inbox", post(http_post_user_inbox))
        .layer(
            ServiceBuilder::new()
                .map_request_body(body::boxed)
                .layer(middleware::from_fn(verify_request_payload)),
        )
        .route("/objects/:user_name", get(http_get_user))
        .layer(ApubMiddleware::new(data))
        .layer(TraceLayer::new_for_http());

    // run it
    let addr = hostname
        .to_socket_addrs()?
        .next()
        .expect("Failed to lookup domain name");
    let server = axum::Server::bind(&addr).serve(app.into_make_service());

    task::spawn(server);
    Ok(())
}

async fn http_get_user(
    data: RequestData<DatabaseHandle>,
    request: Request<Body>,
) -> Result<ApubJson<WithContext<Person>>, Error> {
    let hostname: String = data.local_instance().hostname().to_string();
    let request_url = format!("http://{}{}", hostname, &request.uri());

    let url = Url::parse(&request_url).expect("Failed to parse url");

    let user = ObjectId::<MyUser>::new(url)
        .dereference_local(&data)
        .await?
        .into_apub(&data)
        .await?;

    Ok(ApubJson(WithContext::new_default(user)))
}

async fn http_post_user_inbox(
    headers: HeaderMap,
    method: Method,
    OriginalUri(uri): OriginalUri,
    data: RequestData<DatabaseHandle>,
    Extension(digest_verified): Extension<DigestVerified>,
    Json(activity): Json<WithContext<PersonAcceptedActivities>>,
) -> impl IntoResponse {
    receive_activity::<WithContext<PersonAcceptedActivities>, MyUser, DatabaseHandle>(
        digest_verified,
        activity,
        &data,
        headers,
        method,
        uri,
    )
    .await
}
