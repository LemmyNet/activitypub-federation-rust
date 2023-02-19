use axum::{
    async_trait,
    body::{Bytes, HttpBody},
    extract::FromRequest,
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
};
use http::{HeaderMap, Method, Uri};

pub mod inbox;
pub mod json;
pub mod middleware;

/// Contains everything that is necessary to verify HTTP signatures and receive an
/// activity, including the request body.
#[derive(Debug)]
pub struct ActivityData {
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    body: Vec<u8>,
}

#[async_trait]
impl<S, B> FromRequest<S, B> for ActivityData
where
    Bytes: FromRequest<S, B>,
    B: HttpBody + Send + 'static,
    S: Send + Sync,
    <B as HttpBody>::Error: std::fmt::Display,
    <B as HttpBody>::Data: Send,
{
    type Rejection = Response;

    async fn from_request(req: Request<B>, _state: &S) -> Result<Self, Self::Rejection> {
        let (parts, body) = req.into_parts();

        // this wont work if the body is an long running stream
        let bytes = hyper::body::to_bytes(body)
            .await
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response())?;

        Ok(Self {
            headers: parts.headers,
            method: parts.method,
            uri: parts.uri,
            body: bytes.to_vec(),
        })
    }
}
