use crate::config::{Data, FederationConfig, FederationMiddleware};
use axum::{body::Body, extract::FromRequestParts, http::Request, response::Response};
use http::{request::Parts, StatusCode};
use std::task::{Context, Poll};
use tower::{Layer, Service};

impl<S, T: Clone> Layer<S> for FederationMiddleware<T> {
    type Service = FederationService<S, T>;

    fn layer(&self, inner: S) -> Self::Service {
        FederationService {
            inner,
            config: self.0.clone(),
        }
    }
}

/// Passes [FederationConfig] to HTTP handlers, converting it to [Data] in the process
#[doc(hidden)]
#[derive(Clone)]
pub struct FederationService<S, T: Clone> {
    inner: S,
    config: FederationConfig<T>,
}

impl<S, T> Service<Request<Body>> for FederationService<S, T>
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

impl<S, T: Clone + 'static> FromRequestParts<S> for Data<T>
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
                "Missing extension, did you register FederationMiddleware?",
            )),
        }
    }
}
