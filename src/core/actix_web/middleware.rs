use crate::config::{ApubMiddleware, FederationConfig, RequestData};
use actix_web::{
    dev::{forward_ready, Payload, Service, ServiceRequest, ServiceResponse, Transform},
    Error,
    FromRequest,
    HttpMessage,
    HttpRequest,
};
use std::future::{ready, Ready};

impl<S, B, T> Transform<S, ServiceRequest> for ApubMiddleware<T>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
    T: Clone + Sync + 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Transform = ApubService<S, T>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(ApubService {
            service,
            config: self.0.clone(),
        }))
    }
}

pub struct ApubService<S, T: Clone>
where
    S: Service<ServiceRequest, Error = Error>,
    S::Future: 'static,
    T: Sync,
{
    service: S,
    config: FederationConfig<T>,
}

impl<S, B, T> Service<ServiceRequest> for ApubService<S, T>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
    T: Clone + Sync + 'static,
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

impl<T: Clone + 'static> FromRequest for RequestData<T> {
    type Error = Error;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        ready(match req.extensions().get::<FederationConfig<T>>() {
            Some(c) => Ok(c.to_request_data()),
            None => Err(actix_web::error::ErrorBadRequest(
                "Missing extension, did you register ApubMiddleware?",
            )),
        })
    }
}
