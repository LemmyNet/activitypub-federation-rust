use std::fmt::{Display, Formatter};

/// Necessary because of this issue: https://github.com/actix/actix-web/issues/1711
#[derive(Debug)]
pub struct Error(anyhow::Error);

#[cfg(feature = "actix")]
impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

impl<T> From<T> for Error
where
    T: Into<anyhow::Error>,
{
    fn from(t: T) -> Self {
        Error(t.into())
    }
}

#[cfg(feature = "actix")]
mod actix {
    use crate::error::Error;
    use actix_web::ResponseError;

    impl ResponseError for Error {}
}

#[cfg(feature = "axum")]
mod axum {
    use axum::response::{IntoResponse, Response};
    use http::StatusCode;

    impl IntoResponse for AppError {
        fn into_response(self) -> Response {
            (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", self.0)).into_response()
        }
    }
}
