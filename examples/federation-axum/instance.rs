use crate::{
    error::Error,
    generate_object_id,
    objects::{
        note::MyPost,
        person::{MyUser, Person, PersonAcceptedActivities},
    },
};
use activitypub_federation::{
    core::{
        axum::{inbox::receive_activity, json::ApubJson, verify_request_payload, DigestVerified},
        object_id::ObjectId,
        signatures::generate_actor_keypair,
    },
    deser::context::WithContext,
    request_data::{ApubContext, ApubMiddleware, RequestData},
    traits::ApubObject,
    FederationSettings,
    InstanceConfig,
    UrlVerifier,
};
use async_trait::async_trait;
use axum::{
    body,
    body::Body,
    extract::{Json, OriginalUri},
    middleware,
    response::IntoResponse,
    routing::{get, post},
    Extension,
    Router,
};
use http::{HeaderMap, Method, Request};
use reqwest::Client;
use std::{
    net::ToSocketAddrs,
    sync::{Arc, Mutex},
};
use tokio::task;
use tower::ServiceBuilder;
use tower_http::{trace::TraceLayer, ServiceBuilderExt};
use url::Url;

pub type DatabaseHandle = Arc<Database>;

/// Our "database" which contains all known posts and users (local and federated)
pub struct Database {
    pub users: Mutex<Vec<MyUser>>,
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

impl Database {
    pub fn new(hostname: String) -> Result<ApubContext<DatabaseHandle>, Error> {
        let settings = FederationSettings::builder()
            .debug(true)
            .url_verifier(Box::new(MyUrlVerifier()))
            .build()?;
        let local_instance =
            InstanceConfig::new(hostname.clone(), Client::default().into(), settings);
        let local_user = MyUser::new(generate_object_id(&hostname)?, generate_actor_keypair()?);
        let instance = Arc::new(Database {
            users: Mutex::new(vec![local_user]),
            posts: Mutex::new(vec![]),
        });
        Ok(ApubContext::new(instance, local_instance))
    }

    pub fn local_user(&self) -> MyUser {
        self.users.lock().unwrap().first().cloned().unwrap()
    }

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
