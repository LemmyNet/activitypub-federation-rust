use crate::{
    config::{Data, FederationConfig, FederationMiddleware},
    queue::ActivityQueue,
};
use axum::{async_trait, body::Body, extract::FromRequestParts, http::Request, response::Response};
use http::{request::Parts, StatusCode};
use std::task::{Context, Poll};
use tower::{Layer, Service};

impl<S, T: Clone, Q: ActivityQueue> Layer<S> for FederationMiddleware<T, Q> {
    type Service = FederationService<S, T, Q>;

    fn layer(&self, inner: S) -> Self::Service {
        FederationService {
            inner,
            config: self.config.clone(),
        }
    }
}

/// Passes [FederationConfig] to HTTP handlers, converting it to [Data] in the process
#[doc(hidden)]
pub struct FederationService<S, T: Clone, Q: ActivityQueue> {
    inner: S,
    config: FederationConfig<T, Q>,
}

impl<S: Clone, T: Clone, Q: ActivityQueue> Clone for FederationService<S, T, Q> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            config: self.config.clone(),
        }
    }
}

impl<S, T, Q> Service<Request<Body>> for FederationService<S, T, Q>
where
    S: Service<Request<Body>, Response = Response> + Send + 'static,
    S::Future: Send + 'static,
    T: Clone + Send + Sync + 'static,
    Q: ActivityQueue + Send + Sync + 'static,
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
impl<S, T: Clone + 'static, Q: ActivityQueue + 'static> FromRequestParts<S> for Data<T, Q>
where
    S: Send + Sync,
    T: Send + Sync,
    Q: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        match parts.extensions.get::<FederationConfig<T, Q>>() {
            Some(c) => Ok(c.to_request_data()),
            None => Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Missing extension, did you register FederationMiddleware?",
            )),
        }
    }
}
