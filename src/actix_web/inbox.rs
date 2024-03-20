//! Handles incoming activities, verifying HTTP signatures and other checks

use crate::{
    config::Data,
    error::Error,
    http_signatures::{verify_body_hash, verify_signature},
    parse_received_activity,
    traits::{ActivityHandler, Actor, Object},
};
use actix_web::{http::header::HeaderMap, web::Bytes, HttpRequest, HttpResponse};
use http::{Method, Uri};
use serde::de::DeserializeOwned;
use tracing::debug;

/// Handles incoming activities, verifying HTTP signatures and other checks
///
/// After successful validation, activities are passed to respective [trait@ActivityHandler].
pub async fn receive_activity<Activity, ActorT, Datatype>(
    request: HttpRequest,
    body: Bytes,
    data: &Data<Datatype>,
) -> Result<HttpResponse, <Activity as ActivityHandler>::Error>
where
    Activity: ActivityHandler<DataType = Datatype> + DeserializeOwned + Send + 'static,
    ActorT: Object<DataType = Datatype> + Actor + Send + 'static,
    for<'de2> <ActorT as Object>::Kind: serde::Deserialize<'de2>,
    <Activity as ActivityHandler>::Error: From<Error> + From<<ActorT as Object>::Error>,
    <ActorT as Object>::Error: From<Error>,
    Datatype: Clone,
{
    receive_activity_parts::<Activity, ActorT, Datatype>(
        (request.headers(), request.method(), request.uri()),
        body,
        data,
    )
    .await
}

/// Same as [receive_activity] but only takes the essential parts of [HttpResponse]. Necessary
/// in some cases because [HttpResponse] is not Sync.
pub async fn receive_activity_parts<Activity, ActorT, Datatype>(
    request_parts: (&HeaderMap, &Method, &Uri),
    body: Bytes,
    data: &Data<Datatype>,
) -> Result<HttpResponse, <Activity as ActivityHandler>::Error>
where
    Activity: ActivityHandler<DataType = Datatype> + DeserializeOwned + Send + 'static,
    ActorT: Object<DataType = Datatype> + Actor + Send + 'static,
    for<'de2> <ActorT as Object>::Kind: serde::Deserialize<'de2>,
    <Activity as ActivityHandler>::Error: From<Error> + From<<ActorT as Object>::Error>,
    <ActorT as Object>::Error: From<Error>,
    Datatype: Clone,
{
    let (headers, method, uri) = request_parts;
    verify_body_hash(headers.get("Digest"), &body)?;

    let (activity, actor) = parse_received_activity::<Activity, ActorT, _>(&body, data).await?;

    verify_signature(headers, method, uri, actor.public_key_pem())?;

    debug!("Receiving activity {}", activity.id().to_string());
    activity.verify(data).await?;
    activity.receive(data).await?;
    Ok(HttpResponse::Ok().finish())
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        activity_sending::generate_request_headers,
        config::FederationConfig,
        fetch::object_id::ObjectId,
        http_signatures::sign_request,
        traits::tests::{DbConnection, DbUser, Follow, DB_USER_KEYPAIR},
    };
    use actix_web::test::TestRequest;
    use reqwest::Client;
    use reqwest_middleware::ClientWithMiddleware;
    use serde_json::json;
    use url::Url;

    #[tokio::test]
    async fn test_receive_activity() {
        let (body, incoming_request, config) = setup_receive_test().await;
        receive_activity::<Follow, DbUser, DbConnection>(
            incoming_request.to_http_request(),
            body,
            &config.to_request_data(),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn test_receive_activity_invalid_body_signature() {
        let (_, incoming_request, config) = setup_receive_test().await;
        let err = receive_activity::<Follow, DbUser, DbConnection>(
            incoming_request.to_http_request(),
            "invalid".into(),
            &config.to_request_data(),
        )
        .await
        .err()
        .unwrap();

        assert_eq!(&err, &Error::ActivityBodyDigestInvalid)
    }

    #[tokio::test]
    async fn test_receive_activity_invalid_path() {
        let (body, incoming_request, config) = setup_receive_test().await;
        let incoming_request = incoming_request.uri("/wrong");
        let err = receive_activity::<Follow, DbUser, DbConnection>(
            incoming_request.to_http_request(),
            body,
            &config.to_request_data(),
        )
        .await
        .err()
        .unwrap();

        assert_eq!(&err, &Error::ActivitySignatureInvalid)
    }

    #[tokio::test]
    async fn test_receive_unparseable_activity() {
        let (_, _, config) = setup_receive_test().await;

        let actor = Url::parse("http://ds9.lemmy.ml/u/lemmy_alpha").unwrap();
        let id = "http://localhost:123/1";
        let activity = json!({
          "actor": actor.as_str(),
          "to": ["https://www.w3.org/ns/activitystreams#Public"],
          "object": "http://ds9.lemmy.ml/post/1",
          "cc": ["http://enterprise.lemmy.ml/c/main"],
          "type": "Delete",
          "id": id
        }
        );
        let body: Bytes = serde_json::to_vec(&activity).unwrap().into();
        let incoming_request = construct_request(&body, &actor).await;

        // intentionally cause a parse error by using wrong type for deser
        let res = receive_activity::<Follow, DbUser, DbConnection>(
            incoming_request.to_http_request(),
            body,
            &config.to_request_data(),
        )
        .await;

        match res {
            Err(Error::ParseReceivedActivity(_, url)) => {
                assert_eq!(id, url.expect("has url").as_str());
            }
            _ => unreachable!(),
        }
    }

    async fn construct_request(body: &Bytes, actor: &Url) -> TestRequest {
        let inbox = "https://example.com/inbox";
        let headers = generate_request_headers(&Url::parse(inbox).unwrap());
        let request_builder = ClientWithMiddleware::from(Client::default())
            .post(inbox)
            .headers(headers);
        let outgoing_request = sign_request(
            request_builder,
            actor,
            body.clone(),
            DB_USER_KEYPAIR.private_key().unwrap(),
            false,
        )
        .await
        .unwrap();
        let mut incoming_request = TestRequest::post().uri(outgoing_request.url().path());
        for h in outgoing_request.headers() {
            incoming_request = incoming_request.append_header(h);
        }
        incoming_request
    }

    async fn setup_receive_test() -> (Bytes, TestRequest, FederationConfig<DbConnection>) {
        let activity = Follow {
            actor: ObjectId::parse("http://localhost:123").unwrap(),
            object: ObjectId::parse("http://localhost:124").unwrap(),
            kind: Default::default(),
            id: "http://localhost:123/1".try_into().unwrap(),
        };
        let body: Bytes = serde_json::to_vec(&activity).unwrap().into();
        let incoming_request = construct_request(&body, activity.actor.inner()).await;

        let config = FederationConfig::builder()
            .domain("localhost:8002")
            .app_data(DbConnection)
            .debug(true)
            .build()
            .await
            .unwrap();
        (body, incoming_request, config)
    }
}
