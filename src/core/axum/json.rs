use crate::APUB_JSON_CONTENT_TYPE;
use axum::response::IntoResponse;
use http::header;
use serde::Serialize;

/// A wrapper struct to respond with [`APUB_JSON_CONTENT_TYPE`]
/// in axum handlers
///
/// ## Example:
/// ```rust, no_run
/// use activitypub_federation::deser::context::WithContext;
///  async fn http_get_user() -> Result<ApubJson<WithContext<Person>>, Error> {
///     let user = WithContext::new_default(M);
///
///     Ok(ApubJson(WithContext::new_default(MyUser::default())))
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
                .ok()
                .expect("'Parsing application/activity+json' should never fail"),
        );
        response
    }
}
