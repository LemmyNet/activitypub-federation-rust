use crate::APUB_JSON_CONTENT_TYPE;
use axum::response::IntoResponse;
use http::header;
use serde::Serialize;

/// Wrapper struct to respond with `application/activity+json` in axum handlers
///
/// ```
/// # use anyhow::Error;
/// # use axum::extract::Path;
/// # use activitypub_federation::core::axum::json::ApubJson;
/// # use activitypub_federation::protocol::context::WithContext;
/// # use activitypub_federation::config::RequestData;
/// # use activitypub_federation::traits::ApubObject;
/// # use activitypub_federation::traits::tests::{DbConnection, DbUser, Person};
/// async fn http_get_user(Path(name): Path<String>, data: RequestData<DbConnection>) -> Result<ApubJson<WithContext<Person>>, Error> {
///     let user: DbUser = data.read_local_user(name).await?;
///     let person = user.into_apub(&data).await?;
///
///     Ok(ApubJson(WithContext::new_default(person)))
/// }
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct ApubJson<Json: Serialize>(pub Json);

impl<Json: Serialize> IntoResponse for ApubJson<Json> {
    fn into_response(self) -> axum::response::Response {
        let mut response = axum::response::Json(self.0).into_response();
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            APUB_JSON_CONTENT_TYPE
                .parse()
                .expect("Parsing 'application/activity+json' should never fail"),
        );
        response
    }
}
