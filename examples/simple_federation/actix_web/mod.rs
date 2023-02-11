use crate::error::Error;
use actix_web::ResponseError;

pub(crate) mod http;

impl ResponseError for Error {}
