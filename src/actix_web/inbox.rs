//! Handles incoming activities, verifying HTTP signatures and other checks

use super::http_compat;
use crate::{
    config::Data,
    error::Error,
    http_signatures::{verify_body_hash, verify_signature},
    parse_received_activity,
    traits::{Activity, Actor, Object},
};
use actix_web::{web::Bytes, HttpRequest, HttpResponse};
use serde::de::DeserializeOwned;
use tracing::debug;

/// Handles incoming activities, verifying HTTP signatures and other checks
///
/// After successful validation, activities are passed to respective [trait@Activity].
pub async fn receive_activity<A, ActorT, Datatype>(
    request: HttpRequest,
    body: Bytes,
    data: &Data<Datatype>,
) -> Result<HttpResponse, <A as Activity>::Error>
where
    A: Activity<DataType = Datatype> + DeserializeOwned + Send + 'static,
    ActorT: Object<DataType = Datatype> + Actor + Send + Sync + 'static,
    for<'de2> <ActorT as Object>::Kind: serde::Deserialize<'de2>,
    <A as Activity>::Error: From<Error> + From<<ActorT as Object>::Error>,
    <ActorT as Object>::Error: From<Error>,
    Datatype: Clone,
{
    let (activity, _) = do_stuff::<A, ActorT, Datatype>(request, body, data).await?;

    do_more_stuff(activity, data).await
}

/// Workaround required so we can use references for the hook, instead of cloning data.
pub trait ReceiveActivityHook<A, ActorT, Datatype>
where
    A: Activity<DataType = Datatype> + DeserializeOwned + Send + Clone + 'static,
    ActorT: Object<DataType = Datatype> + Actor + Send + Clone + 'static,
    for<'de2> <ActorT as Object>::Kind: serde::Deserialize<'de2>,
    <A as Activity>::Error: From<Error> + From<<ActorT as Object>::Error>,
    <ActorT as Object>::Error: From<Error>,
    Datatype: Clone,
{
    /// Called when a new activity is recived
    fn hook(
        self,
        activity: &A,
        actor: &ActorT,
        data: &Data<Datatype>,
    ) -> impl std::future::Future<Output = Result<(), <A as Activity>::Error>>;
}

/// Same as [receive_activity], only that it calls the provided hook function before
/// calling activity verify and receive functions.
pub async fn receive_activity_with_hook<A, ActorT, Datatype>(
    request: HttpRequest,
    body: Bytes,
    hook: impl ReceiveActivityHook<A, ActorT, Datatype>,
    data: &Data<Datatype>,
) -> Result<HttpResponse, <A as Activity>::Error>
where
    A: Activity<DataType = Datatype> + DeserializeOwned + Send + Clone + 'static,
    ActorT: Object<DataType = Datatype> + Actor + Send + Sync + Clone + 'static,
    for<'de2> <ActorT as Object>::Kind: serde::Deserialize<'de2>,
    <A as Activity>::Error: From<Error> + From<<ActorT as Object>::Error>,
    <ActorT as Object>::Error: From<Error>,
    Datatype: Clone,
{
    let (activity, actor) = do_stuff::<A, ActorT, Datatype>(request, body, data).await?;

    hook.hook(&activity, &actor, data).await?;

    do_more_stuff(activity, data).await
}

async fn do_stuff<A, ActorT, Datatype>(
    request: HttpRequest,
    body: Bytes,
    data: &Data<Datatype>,
) -> Result<(A, ActorT), <A as Activity>::Error>
where
    A: Activity<DataType = Datatype> + DeserializeOwned + Send + 'static,
    ActorT: Object<DataType = Datatype> + Actor + Send + Sync + 'static,
    for<'de2> <ActorT as Object>::Kind: serde::Deserialize<'de2>,
    <A as Activity>::Error: From<Error> + From<<ActorT as Object>::Error>,
    <ActorT as Object>::Error: From<Error>,
    Datatype: Clone,
{
    let digest_header = request
        .headers()
        .get("Digest")
        .map(http_compat::header_value);
    verify_body_hash(digest_header.as_ref(), &body)?;

    let (activity, actor) = parse_received_activity::<A, ActorT, _>(&body, data).await?;

    let headers = http_compat::header_map(request.headers());
    let method = http_compat::method(request.method());
    let uri = http_compat::uri(request.uri());
    verify_signature(&headers, &method, &uri, actor.public_key_pem())?;

    Ok((activity, actor))
}

async fn do_more_stuff<A, Datatype>(
    activity: A,
    data: &Data<Datatype>,
) -> Result<HttpResponse, <A as Activity>::Error>
where
    A: Activity<DataType = Datatype> + DeserializeOwned + Send + 'static,
    Datatype: Clone,
{
    debug!("Receiving activity {}", activity.id().to_string());
    activity.verify(data).await?;
    activity.receive(data).await?;
    Ok(HttpResponse::Ok().finish())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
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

    /// Remove this conversion helper after actix-web upgrades to http 1.0
    fn header_pair(
        p: (&http::HeaderName, &http::HeaderValue),
    ) -> (http02::HeaderName, http02::HeaderValue) {
        (
            http02::HeaderName::from_lowercase(p.0.as_str().as_bytes()).unwrap(),
            http02::HeaderValue::from_bytes(p.1.as_bytes()).unwrap(),
        )
    }

    #[tokio::test]
    async fn test_receive_activity_hook() {
        let (body, incoming_request, config) = setup_receive_test().await;
        let res = receive_activity_with_hook::<Follow, DbUser, DbConnection>(
            incoming_request.to_http_request(),
            body,
            Dummy,
            &config.to_request_data(),
        )
        .await;
        assert_eq!(res.err(), Some(Error::Other("test-error".to_string())));
    }

    struct Dummy;

    impl<A, ActorT, Datatype> ReceiveActivityHook<A, ActorT, Datatype> for Dummy
    where
        A: Activity<DataType = Datatype> + DeserializeOwned + Send + Clone + 'static,
        ActorT: Object<DataType = Datatype> + Actor + Send + Clone + 'static,
        for<'de2> <ActorT as Object>::Kind: serde::Deserialize<'de2>,
        <A as Activity>::Error: From<Error> + From<<ActorT as Object>::Error>,
        <ActorT as Object>::Error: From<Error>,
        Datatype: Clone,
    {
        async fn hook(
            self,
            _activity: &A,
            _actor: &ActorT,
            _data: &Data<Datatype>,
        ) -> Result<(), <A as Activity>::Error> {
            // ensure that hook gets called by returning this value
            Err(Error::Other("test-error".to_string()).into())
        }
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
        let activity_id = "http://localhost:123/1";
        let activity = json!({
          "actor": actor.as_str(),
          "to": ["https://www.w3.org/ns/activitystreams#Public"],
          "object": "http://ds9.lemmy.ml/post/1",
          "cc": ["http://enterprise.lemmy.ml/c/main"],
          "type": "Delete",
          "id": activity_id
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
            Err(Error::ParseReceivedActivity { err: _, id }) => {
                assert_eq!(activity_id, id.expect("has url").as_str());
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
            incoming_request = incoming_request.append_header(header_pair(h));
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
