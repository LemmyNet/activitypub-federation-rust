#![doc = include_str!("../docs/01_intro.md")]
#![doc = include_str!("../docs/02_overview.md")]
#![doc = include_str!("../docs/03_federating_users.md")]
#![doc = include_str!("../docs/04_federating_posts.md")]
#![doc = include_str!("../docs/05_configuration.md")]
#![doc = include_str!("../docs/06_http_endpoints_axum.md")]
#![doc = include_str!("../docs/07_fetching_data.md")]
#![doc = include_str!("../docs/08_receiving_activities.md")]
#![doc = include_str!("../docs/09_sending_activities.md")]
#![doc = include_str!("../docs/10_fetching_objects_with_unknown_type.md")]
#![deny(missing_docs)]

#[cfg(feature = "actix-web")]
pub mod actix_web;
#[cfg(feature = "axum")]
pub mod axum;
pub mod config;
pub mod error;
pub mod fetch;
pub mod http_signatures;
pub mod protocol;
pub mod queue;
pub(crate) mod reqwest_shim;
pub mod traits;

pub use activitystreams_kinds as kinds;

/// Mime type for Activitypub data, used for `Accept` and `Content-Type` HTTP headers
pub static FEDERATION_CONTENT_TYPE: &str = "application/activity+json";
