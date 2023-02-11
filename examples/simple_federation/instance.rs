use crate::{
    generate_object_id,
    objects::{note::MyPost, person::MyUser},
    Error,
};
use activitypub_federation::{
    core::signatures::generate_actor_keypair,
    request_data::ApubContext,
    FederationSettings,
    InstanceConfig,
    UrlVerifier,
};
use async_trait::async_trait;
use reqwest::Client;
use std::sync::{Arc, Mutex};
use url::Url;

pub type DatabaseHandle = Arc<Database>;

/// Our "database" which contains all known posts users (local and federated)
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

pub fn listen(data: &ApubContext<DatabaseHandle>) -> Result<(), Error> {
    #[cfg(feature = "actix-web")]
    crate::actix_web::http::listen(data)?;
    #[cfg(feature = "axum")]
    crate::axum::http::listen(data)?;
    Ok(())
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
}
