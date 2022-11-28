use crate::{
    error::Error,
    generate_object_id,
    objects::{
        note::MyPost,
        person::{MyUser, PersonAcceptedActivities},
    },
};

use activitypub_federation::{
    core::{
        actix::inbox::receive_activity,
        object_id::ObjectId,
        signatures::generate_actor_keypair,
    },
    data::Data,
    deser::context::WithContext,
    traits::ApubObject,
    InstanceSettings,
    LocalInstance,
    UrlVerifier,
    APUB_JSON_CONTENT_TYPE,
};

use actix_web::{web, App, HttpRequest, HttpResponse, HttpServer};
use async_trait::async_trait;
use reqwest::Client;
use std::{
    ops::Deref,
    sync::{Arc, Mutex},
};
use tokio::task;
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
        let server = HttpServer::new(move || {
            App::new()
                .app_data(web::Data::new(instance.clone()))
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
}

/// Handles requests to fetch user json over HTTP
async fn http_get_user(
    request: HttpRequest,
    data: web::Data<InstanceHandle>,
) -> Result<HttpResponse, Error> {
    let data: InstanceHandle = data.into_inner().deref().clone();
    let hostname: String = data.local_instance.hostname().to_string();
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
async fn http_post_user_inbox(
    request: HttpRequest,
    payload: String,
    data: web::Data<InstanceHandle>,
) -> Result<HttpResponse, Error> {
    let data: InstanceHandle = data.into_inner().deref().clone();
    let activity = serde_json::from_str(&payload)?;
    receive_activity::<WithContext<PersonAcceptedActivities>, MyUser, InstanceHandle>(
        request,
        activity,
        &data.clone().local_instance,
        &Data::new(data),
    )
    .await
}
