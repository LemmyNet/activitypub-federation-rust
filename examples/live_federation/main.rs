#![allow(clippy::unwrap_used)]

use crate::{
    database::Database,
    http::{http_get_user, http_post_user_inbox, webfinger},
    objects::{person::DbUser, post::DbPost},
    utils::generate_object_id,
};
use activitypub_federation::config::{FederationConfig, FederationMiddleware};
use axum::{
    routing::{get, post},
    Router,
};
use error::Error;
use std::{
    net::ToSocketAddrs,
    sync::{Arc, Mutex},
};
use tracing::log::{info, LevelFilter};

mod activities;
mod database;
mod error;
#[allow(clippy::diverging_sub_expression, clippy::items_after_statements)]
mod http;
mod objects;
mod utils;

const DOMAIN: &str = "example.com";
const LOCAL_USER_NAME: &str = "alison";
const BIND_ADDRESS: &str = "localhost:8003";

#[tokio::main]
async fn main() -> Result<(), Error> {
    env_logger::builder()
        .filter_level(LevelFilter::Warn)
        .filter_module("activitypub_federation", LevelFilter::Info)
        .filter_module("live_federation", LevelFilter::Info)
        .format_timestamp(None)
        .init();

    info!("Setup local user and database");
    let local_user = DbUser::new(DOMAIN, LOCAL_USER_NAME)?;
    let database = Arc::new(Database {
        users: Mutex::new(vec![local_user]),
    });

    info!("Setup configuration");
    let config = FederationConfig::builder()
        .domain(DOMAIN)
        .app_data(database)
        .build()
        .await?;

    info!("Listen with HTTP server on {BIND_ADDRESS}");
    let config = config.clone();
    let app = Router::new()
        .route("/{user}", get(http_get_user))
        .route("/{user}/inbox", post(http_post_user_inbox))
        .route("/.well-known/webfinger", get(webfinger))
        .layer(FederationMiddleware::new(config));

    let addr = BIND_ADDRESS
        .to_socket_addrs()?
        .next()
        .expect("Failed to lookup domain name");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}
