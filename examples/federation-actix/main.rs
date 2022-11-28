use crate::{error::Error, instance::Instance, objects::note::MyPost, utils::generate_object_id};
use tracing::log::LevelFilter;

mod activities;
mod error;
mod instance;
mod objects;
mod utils;

#[actix_rt::main]
async fn main() -> Result<(), Error> {
    env_logger::builder()
        .filter_level(LevelFilter::Debug)
        .init();

    let alpha = Instance::new("localhost:8001".to_string())?;
    let beta = Instance::new("localhost:8002".to_string())?;
    Instance::listen(&alpha)?;
    Instance::listen(&beta)?;

    // alpha user follows beta user
    alpha
        .local_user()
        .follow(&beta.local_user(), &alpha)
        .await?;
    // assert that follow worked correctly
    assert_eq!(
        beta.local_user().followers(),
        &vec![alpha.local_user().ap_id.inner().clone()]
    );

    // beta sends a post to its followers
    let sent_post = MyPost::new("hello world!".to_string(), beta.local_user().ap_id);
    beta.local_user().post(sent_post.clone(), &beta).await?;
    let received_post = alpha.posts.lock().unwrap().first().cloned().unwrap();

    // assert that alpha received the post
    assert_eq!(received_post.text, sent_post.text);
    assert_eq!(received_post.ap_id.inner(), sent_post.ap_id.inner());
    assert_eq!(received_post.creator.inner(), sent_post.creator.inner());
    Ok(())
}
