use crate::{
    config::{Data, FederationConfig, FederationMiddleware},
    queue::ActivityQueue,
};
use actix_web::{
    dev::{forward_ready, Payload, Service, ServiceRequest, ServiceResponse, Transform},
    Error,
    FromRequest,
    HttpMessage,
    HttpRequest,
};
use std::future::{ready, Ready};

impl<S, B, T, Q> Transform<S, ServiceRequest> for FederationMiddleware<T, Q>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
    T: Clone + Sync + 'static,
    Q: ActivityQueue + Sync + 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Transform = FederationService<S, T, Q>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(FederationService {
            service,
            config: self.config.clone(),
        }))
    }
}

/// Passes [FederationConfig] to HTTP handlers, converting it to [Data] in the process
#[doc(hidden)]
pub struct FederationService<S, T: Clone, Q: ActivityQueue>
where
    S: Service<ServiceRequest, Error = Error>,
    S::Future: 'static,
    T: Sync,
{
    service: S,
    config: FederationConfig<T, Q>,
}

impl<S, B, T, Q> Service<ServiceRequest> for FederationService<S, T, Q>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
    T: Clone + Sync + 'static,
    Q: ActivityQueue + Sync + 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = S::Future;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        req.extensions_mut().insert(self.config.clone());

        self.service.call(req)
    }
}

impl<T: Clone + 'static, Q: ActivityQueue + 'static> FromRequest for Data<T, Q> {
    type Error = Error;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        ready(match req.extensions().get::<FederationConfig<T, Q>>() {
            Some(c) => Ok(c.to_request_data()),
            None => Err(actix_web::error::ErrorBadRequest(
                "Missing extension, did you register FederationMiddleware?",
            )),
        })
    }
}
