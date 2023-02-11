use crate::error::Error;
use ::http::StatusCode;
use axum::response::{IntoResponse, Response};

pub mod http;

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", self.0)).into_response()
    }
}
