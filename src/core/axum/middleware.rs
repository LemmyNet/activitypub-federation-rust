use crate::config::{ApubMiddleware, FederationConfig, RequestData};
use axum::{async_trait, body::Body, extract::FromRequestParts, http::Request, response::Response};
use http::{request::Parts, StatusCode};
use std::task::{Context, Poll};
use tower::{Layer, Service};

impl<S, T: Clone> Layer<S> for ApubMiddleware<T> {
    type Service = ApubService<S, T>;

    fn layer(&self, inner: S) -> Self::Service {
        ApubService {
            inner,
            config: self.0.clone(),
        }
    }
}

/// Passes [FederationConfig] to HTTP handlers, converting it to [RequestData] in the process
#[doc(hidden)]
#[derive(Clone)]
pub struct ApubService<S, T: Clone> {
    inner: S,
    config: FederationConfig<T>,
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
        request.extensions_mut().insert(self.config.clone());
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
        match parts.extensions.get::<FederationConfig<T>>() {
            Some(c) => Ok(c.to_request_data()),
            None => Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Missing extension, did you register ApubMiddleware?",
            )),
        }
    }
}
