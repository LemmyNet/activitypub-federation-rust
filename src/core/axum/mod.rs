use axum::{
    async_trait,
    body::{self, BoxBody, Bytes, Full},
    extract::FromRequest,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use digest::{verify_sha256, DigestPart};

mod digest;
pub mod inbox;
pub mod json;
pub mod middleware;

/// A request guard to ensure digest has been verified request has been
/// see [`receive_activity`]
#[derive(Clone)]
pub struct DigestVerified;

pub struct BufferRequestBody(pub Bytes);

pub async fn verify_request_payload(
    request: Request<BoxBody>,
    next: Next<BoxBody>,
) -> Result<impl IntoResponse, Response> {
    let mut request = verify_payload(request).await?;
    request.extensions_mut().insert(DigestVerified);
    Ok(next.run(request).await)
}

async fn verify_payload(request: Request<BoxBody>) -> Result<Request<BoxBody>, Response> {
    let (parts, body) = request.into_parts();

    // this wont work if the body is an long running stream
    let bytes = hyper::body::to_bytes(body)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response())?;

    match parts.headers.get("Digest") {
        None => Err((
            StatusCode::UNAUTHORIZED,
            "Missing digest header".to_string(),
        )
            .into_response()),
        Some(digest) => match DigestPart::try_from_header(digest) {
            None => Err((
                StatusCode::UNAUTHORIZED,
                "Malformed digest header".to_string(),
            )
                .into_response()),
            Some(digests) => {
                if !verify_sha256(&digests, bytes.as_ref()) {
                    Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Digest does not match payload".to_string(),
                    )
                        .into_response())
                } else {
                    Ok(Request::from_parts(parts, body::boxed(Full::from(bytes))))
                }
            }
        },
    }
}

#[async_trait]
impl<S> FromRequest<S, BoxBody> for BufferRequestBody
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request(req: Request<BoxBody>, state: &S) -> Result<Self, Self::Rejection> {
        let body = Bytes::from_request(req, state)
            .await
            .map_err(IntoResponse::into_response)?;

        Ok(Self(body))
    }
}
