use crate::{
    objects::{person::DbUser, post::DbPost},
    Error,
};
use activitypub_federation::config::{FederationConfig, UrlVerifier};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use url::Url;

pub type DatabaseHandle = Arc<Database>;

/// Our "database" which contains all known posts users (local and federated)
pub struct Database {
    pub users: Mutex<Vec<DbUser>>,
    pub posts: Mutex<Vec<DbPost>>,
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

pub fn listen(data: &FederationConfig<DatabaseHandle>) -> Result<(), Error> {
    if cfg!(feature = "actix-web") == cfg!(feature = "axum") {
        panic!("Exactly one of features \"actix-web\" and \"axum\" must be enabled");
    }
    #[cfg(feature = "actix-web")]
    crate::actix_web::http::listen(data)?;
    #[cfg(feature = "axum")]
    crate::axum::http::listen(data)?;
    Ok(())
}

impl Database {
    pub fn new(hostname: &str, name: String) -> Result<FederationConfig<DatabaseHandle>, Error> {
        let local_user = DbUser::new(hostname, name)?;
        let database = Arc::new(Database {
            users: Mutex::new(vec![local_user]),
            posts: Mutex::new(vec![]),
        });
        let config = FederationConfig::builder()
            .hostname(hostname)
            .app_data(database)
            .debug(true)
            .build()?;
        Ok(config)
    }

    pub fn local_user(&self) -> DbUser {
        let lock = self.users.lock().unwrap();
        lock.first().unwrap().clone()
    }
}
