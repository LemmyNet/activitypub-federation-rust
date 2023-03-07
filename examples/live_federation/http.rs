use crate::{
    database::DatabaseHandle,
    error::Error,
    objects::person::{DbUser, Person, PersonAcceptedActivities},
};
use activitypub_federation::{
    axum::{
        inbox::{receive_activity, ActivityData},
        json::ApubJson,
    },
    config::RequestData,
    fetch::webfinger::{build_webfinger_response, extract_webfinger_name, Webfinger},
    protocol::context::WithContext,
    traits::ApubObject,
};
use axum::{
    extract::{Path, Query},
    response::{IntoResponse, Response},
    Json,
};
use axum_macros::debug_handler;
use http::StatusCode;
use serde::Deserialize;

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", self.0)).into_response()
    }
}

#[debug_handler]
pub async fn http_get_user(
    Path(name): Path<String>,
    data: RequestData<DatabaseHandle>,
) -> Result<ApubJson<WithContext<Person>>, Error> {
    let db_user = data.read_user(&name)?;
    let apub_user = db_user.into_apub(&data).await?;
    Ok(ApubJson(WithContext::new_default(apub_user)))
}

#[debug_handler]
pub async fn http_post_user_inbox(
    data: RequestData<DatabaseHandle>,
    activity_data: ActivityData,
) -> impl IntoResponse {
    receive_activity::<WithContext<PersonAcceptedActivities>, DbUser, DatabaseHandle>(
        activity_data,
        &data,
    )
    .await
}

#[derive(Deserialize)]
pub struct WebfingerQuery {
    resource: String,
}

#[debug_handler]
pub async fn webfinger(
    Query(query): Query<WebfingerQuery>,
    data: RequestData<DatabaseHandle>,
) -> Result<Json<Webfinger>, Error> {
    let name = extract_webfinger_name(&query.resource, &data)?;
    let db_user = data.read_user(&name)?;
    Ok(Json(build_webfinger_response(
        query.resource,
        db_user.ap_id.into_inner(),
    )))
}
