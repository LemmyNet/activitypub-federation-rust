use crate::{error::Error, instance::Database, objects::note::MyPost, utils::generate_object_id};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod activities;
mod error;
mod instance;
mod objects;
mod utils;

#[actix_rt::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| {
                "activitypub_federation=debug,federation-axum=debug,tower_http=debug".into()
            }),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let alpha = Database::new("localhost:8001".to_string())?;
    let beta = Database::new("localhost:8002".to_string())?;
    Database::listen(&alpha)?;
    Database::listen(&beta)?;

    // alpha user follows beta user
    alpha
        .local_user()
        .follow(&beta.local_user(), &alpha.to_request_data())
        .await?;

    // assert that follow worked correctly
    assert_eq!(
        beta.local_user().followers(),
        &vec![alpha.local_user().ap_id.inner().clone()]
    );

    // beta sends a post to its followers
    let sent_post = MyPost::new("hello world!".to_string(), beta.local_user().ap_id);
    beta.local_user()
        .post(sent_post.clone(), &beta.to_request_data())
        .await?;
    let received_post = alpha.posts.lock().unwrap().first().cloned().unwrap();

    // assert that alpha received the post
    assert_eq!(received_post.text, sent_post.text);
    assert_eq!(received_post.ap_id.inner(), sent_post.ap_id.inner());
    assert_eq!(received_post.creator.inner(), sent_post.creator.inner());
    Ok(())
}
