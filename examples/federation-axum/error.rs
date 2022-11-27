/// Necessary because of this issue: https://github.com/actix/actix-web/issues/1711
#[derive(Debug)]
pub struct Error(anyhow::Error);

impl<T> From<T> for Error
where
    T: Into<anyhow::Error>,
{
    fn from(t: T) -> Self {
        Error(t.into())
    }
}

mod axum {
    use super::Error;
    use axum::response::{IntoResponse, Response};
    use http::StatusCode;

    impl IntoResponse for Error {
        fn into_response(self) -> Response {
            (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", self.0)).into_response()
        }
    }
}
