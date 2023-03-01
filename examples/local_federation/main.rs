use crate::{
    instance::{listen, new_instance},
    objects::post::DbPost,
    utils::generate_object_id,
};
use error::Error;
use tracing::log::{info, LevelFilter};

mod activities;
#[cfg(feature = "actix-web")]
mod actix_web;
#[cfg(feature = "axum")]
mod axum;
mod error;
mod instance;
mod objects;
mod utils;

#[actix_rt::main]
async fn main() -> Result<(), Error> {
    env_logger::builder()
        .filter_level(LevelFilter::Warn)
        .filter_module("local_federation", LevelFilter::Info)
        .format_timestamp(None)
        .init();

    info!("Starting local instances alpha and beta on localhost:8001, localhost:8002");
    let alpha = new_instance("localhost:8001", "alpha".to_string())?;
    let beta = new_instance("localhost:8002", "beta".to_string())?;
    listen(&alpha)?;
    listen(&beta)?;
    info!("Local instances started");

    info!("Alpha user follows beta user via webfinger");
    alpha
        .local_user()
        .follow("beta@localhost:8002", &alpha.to_request_data())
        .await?;
    assert_eq!(
        beta.local_user().followers(),
        &vec![alpha.local_user().ap_id.inner().clone()]
    );
    info!("Follow was successful");

    info!("Beta sends a post to its followers");
    let sent_post = DbPost::new("Hello world!".to_string(), beta.local_user().ap_id)?;
    beta.local_user()
        .post(sent_post.clone(), &beta.to_request_data())
        .await?;
    let received_post = alpha.posts.lock().unwrap().first().cloned().unwrap();
    info!("Alpha received post: {}", received_post.text);

    // assert that alpha received the post
    assert_eq!(received_post.text, sent_post.text);
    assert_eq!(received_post.ap_id.inner(), sent_post.ap_id.inner());
    assert_eq!(received_post.creator.inner(), sent_post.creator.inner());
    info!("Test completed");
    Ok(())
}
