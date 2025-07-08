//! Generate HTTP responses for Activitypub ojects

use crate::{
    protocol::{context::WithContext, tombstone::Tombstone},
    FEDERATION_CONTENT_TYPE,
};
use actix_web::HttpResponse;
use http02::header::VARY;
use serde::Serialize;
use serde_json::Value;
use url::Url;

/// TODO
pub fn create_http_response<T: Serialize>(
    data: T,
    federation_context: &Value,
) -> Result<HttpResponse, serde_json::Error> {
    let json = serde_json::to_string_pretty(&WithContext::new(data, federation_context.clone()))?;

    Ok(HttpResponse::Ok()
        .content_type(FEDERATION_CONTENT_TYPE)
        .insert_header((VARY, "Accept"))
        .body(json))
}

pub(crate) fn create_tombstone_response(
    id: Url,
    federation_context: &Value,
) -> Result<HttpResponse, serde_json::Error> {
    let tombstone = Tombstone::new(id);
    let json =
        serde_json::to_string_pretty(&WithContext::new(tombstone, federation_context.clone()))?;

    Ok(HttpResponse::Gone()
        .content_type(FEDERATION_CONTENT_TYPE)
        .status(actix_web::http::StatusCode::GONE)
        .insert_header((VARY, "Accept"))
        .body(json))
}

pub(crate) fn redirect_remote_object(url: &Url) -> HttpResponse {
    let mut res = HttpResponse::PermanentRedirect();
    res.insert_header((actix_web::http::header::LOCATION, url.as_str()));
    res.finish()
}
