//! Wrapper struct to respond with `application/activity+json` in axum handlers
//!
//! ```
//! # use anyhow::Error;
//! # use axum::extract::Path;
//! # use activitypub_federation::axum::json::FederationJson;
//! # use activitypub_federation::protocol::context::WithContext;
//! # use activitypub_federation::config::Data;
//! # use activitypub_federation::traits::Object;
//! # use activitypub_federation::traits::tests::{DbConnection, DbUser, Person};
//! async fn http_get_user(Path(name): Path<String>, data: Data<DbConnection>) -> Result<FederationJson<WithContext<Person>>, Error> {
//!     let user: DbUser = data.read_local_user(name).await?;
//!     let person = user.into_json(&data).await?;
//!
//!     Ok(FederationJson(WithContext::new_default(person)))
//! }
//! ```

use crate::FEDERATION_CONTENT_TYPE;
use axum::response::IntoResponse;
use http::header;
use serde::Serialize;

/// Wrapper struct to respond with `application/activity+json` in axum handlers
#[derive(Debug, Clone, Copy, Default)]
pub struct FederationJson<Json: Serialize>(pub Json);

impl<Json: Serialize> IntoResponse for FederationJson<Json> {
    fn into_response(self) -> axum::response::Response {
        let mut response = axum::response::Json(self.0).into_response();
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            FEDERATION_CONTENT_TYPE
                .parse()
                .expect("Parsing 'application/activity+json' should never fail"),
        );
        response
    }
}
