use crate::request_data::{ApubContext, ApubMiddleware, RequestData};
use axum::{async_trait, body::Body, extract::FromRequestParts, http::Request, response::Response};
use http::{request::Parts, StatusCode};
use std::task::{Context, Poll};
use tower::{Layer, Service};

impl<S, T: Clone> Layer<S> for ApubMiddleware<T> {
    type Service = ApubService<S, T>;

    fn layer(&self, inner: S) -> Self::Service {
        ApubService {
            inner,
            context: self.0.clone(),
        }
    }
}

#[derive(Clone)]
pub struct ApubService<S, T> {
    inner: S,
    context: ApubContext<T>,
}

impl<S, T> Service<Request<Body>> for ApubService<S, T>
where
    S: Service<Request<Body>, Response = Response> + Send + 'static,
    S::Future: Send + 'static,
    T: Clone + Send + Sync + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut request: Request<Body>) -> Self::Future {
        request.extensions_mut().insert(self.context.clone());
        self.inner.call(request)
    }
}

#[async_trait]
impl<S, T: Clone + 'static> FromRequestParts<S> for RequestData<T>
where
    S: Send + Sync,
    T: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // TODO: need to set this extension from middleware
        match parts.extensions.get::<ApubContext<T>>() {
            Some(c) => Ok(c.to_request_data()),
            None => Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Missing extension, did you register ApubMiddleware?",
            )),
        }
    }
}
