use crate::{
    error::Error,
    generate_object_id,
    objects::{
        note::MyPost,
        person::{MyUser, PersonAcceptedActivities},
    },
};

use activitypub_federation::{
    core::{object_id::ObjectId, signatures::generate_actor_keypair},
    data::Data,
    deser::context::WithContext,
    traits::ApubObject,
    InstanceSettings,
    LocalInstance,
    UrlVerifier,
    APUB_JSON_CONTENT_TYPE,
};

use activitypub_federation::core::axum::{verify_request_payload, DigestVerified};
use async_trait::async_trait;
use axum::{
    body,
    body::{Body, BoxBody},
    extract::{Json, OriginalUri, State},
    middleware,
    response::IntoResponse,
    routing::{get, post},
    Extension,
    Router,
};
use http::{header::CONTENT_TYPE, HeaderMap, Method, Request, Response};
use reqwest::Client;
use std::{
    net::ToSocketAddrs,
    sync::{Arc, Mutex},
};
use tokio::task;
use tower::ServiceBuilder;
use tower_http::ServiceBuilderExt;
use url::Url;

pub type InstanceHandle = Arc<Instance>;

pub struct Instance {
    /// This holds all library data
    local_instance: LocalInstance,
    /// Our "database" which contains all known users (local and federated)
    pub users: Mutex<Vec<MyUser>>,
    /// Same, but for posts
    pub posts: Mutex<Vec<MyPost>>,
}

/// Use this to store your federation blocklist, or a database connection needed to retrieve it.
#[derive(Clone)]
struct MyUrlVerifier();

#[async_trait]
impl UrlVerifier for MyUrlVerifier {
    async fn verify(&self, url: &Url) -> Result<(), &'static str> {
        if url.domain() == Some("malicious.com") {
            Err("malicious domain")
        } else {
            Ok(())
        }
    }
}

impl Instance {
    pub fn new(hostname: String) -> Result<InstanceHandle, Error> {
        let settings = InstanceSettings::builder()
            .debug(true)
            .url_verifier(Box::new(MyUrlVerifier()))
            .build()?;
        let local_instance =
            LocalInstance::new(hostname.clone(), Client::default().into(), settings);
        let local_user = MyUser::new(generate_object_id(&hostname)?, generate_actor_keypair()?);
        let instance = Arc::new(Instance {
            local_instance,
            users: Mutex::new(vec![local_user]),
            posts: Mutex::new(vec![]),
        });
        Ok(instance)
    }

    pub fn local_user(&self) -> MyUser {
        self.users.lock().unwrap().first().cloned().unwrap()
    }

    pub fn local_instance(&self) -> &LocalInstance {
        &self.local_instance
    }

    pub fn listen(instance: &InstanceHandle) -> Result<(), Error> {
        let hostname = instance.local_instance.hostname();
        let instance = instance.clone();
        let app = Router::new()
            .route("/inbox", post(http_post_user_inbox))
            .layer(
                ServiceBuilder::new()
                    .map_request_body(body::boxed)
                    .layer(middleware::from_fn(verify_request_payload)),
            )
            .route("/objects/:user_name", get(http_get_user))
            .with_state(instance)
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
}

use activitypub_federation::core::axum::inbox::receive_activity;
use tower_http::trace::TraceLayer;

async fn http_get_user(
    State(data): State<InstanceHandle>,
    request: Request<Body>,
) -> impl IntoResponse {
    let hostname: String = data.local_instance.hostname().to_string();
    let request_url = format!("http://{}{}", hostname, &request.uri());

    let url = Url::parse(&request_url).expect("Failed to parse url");

    let user = ObjectId::<MyUser>::new(url)
        .dereference_local(&data)
        .await
        .expect("Failed to dereference user");

    let user = user
        .into_apub(&data)
        .await
        .expect("Failed to convert to apub user");

    let json =
        serde_json::to_string(&WithContext::new_default(user)).expect("failed to parse json");

    Response::builder()
        .header(CONTENT_TYPE, APUB_JSON_CONTENT_TYPE)
        .body(json)
        .expect("failed to build response")
}

async fn http_post_user_inbox(
    headers: HeaderMap,
    method: Method,
    OriginalUri(uri): OriginalUri,
    State(data): State<InstanceHandle>,
    Extension(digest_verified): Extension<DigestVerified>,
    Json(activity): Json<WithContext<PersonAcceptedActivities>>,
) -> impl IntoResponse {
    receive_activity::<WithContext<PersonAcceptedActivities>, MyUser, InstanceHandle>(
        digest_verified,
        activity,
        &data.clone().local_instance,
        &Data::new(data),
        headers,
        method,
        uri,
    )
    .await
}
