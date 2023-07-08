use crate::{
    objects::{person::DbUser, post::DbPost},
    Error,
};
use activitypub_federation::config::{FederationConfig, UrlVerifier};
use anyhow::anyhow;
use async_trait::async_trait;
use std::{
    str::FromStr,
    sync::{Arc, Mutex},
};
use url::Url;

pub async fn new_instance(
    hostname: &str,
    name: String,
) -> Result<FederationConfig<DatabaseHandle>, Error> {
    let mut system_user = DbUser::new(hostname, "system".into())?;
    system_user.ap_id = Url::parse(&format!("http://{}/", hostname))?.into();

    let local_user = DbUser::new(hostname, name)?;
    let database = Arc::new(Database {
        system_user: system_user.clone(),
        users: Mutex::new(vec![local_user]),
        posts: Mutex::new(vec![]),
    });
    let config = FederationConfig::builder()
        .domain(hostname)
        .signed_fetch_actor(&system_user)
        .app_data(database)
        .debug(true)
        .build()
        .await?;
    Ok(config)
}

pub type DatabaseHandle = Arc<Database>;

/// Our "database" which contains all known posts and users (local and federated)
pub struct Database {
    pub system_user: DbUser,
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

pub enum Webserver {
    Axum,
    ActixWeb,
}

impl FromStr for Webserver {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "axum" => Webserver::Axum,
            "actix-web" => Webserver::ActixWeb,
            _ => panic!("Invalid webserver parameter, must be either `axum` or `actix-web`"),
        })
    }
}

pub fn listen(
    config: &FederationConfig<DatabaseHandle>,
    webserver: &Webserver,
) -> Result<(), Error> {
    match webserver {
        Webserver::Axum => crate::axum::http::listen(config)?,
        Webserver::ActixWeb => crate::actix_web::http::listen(config)?,
    }
    Ok(())
}

impl Database {
    pub fn local_user(&self) -> DbUser {
        let lock = self.users.lock().unwrap();
        lock.first().unwrap().clone()
    }

    pub fn read_user(&self, name: &str) -> Result<DbUser, Error> {
        let db_user = self.local_user();
        if name == db_user.name {
            Ok(db_user)
        } else {
            Err(anyhow!("Invalid user {name}").into())
        }
    }
}
