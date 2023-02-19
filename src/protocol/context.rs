use crate::{
    protocol::helpers::deserialize_one_or_many,
    request_data::RequestData,
    traits::ActivityHandler,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::str::FromStr;
use url::Url;

/// Default context used in Activitypub
const DEFAULT_CONTEXT: &str = "[\"https://www.w3.org/ns/activitystreams\"]";

/// Wrapper for federated structs which handles `@context` field.
///
/// This wrapper can be used when sending Activitypub data, to automatically add `@context`. It
/// avoids having to repeat the `@context` property on every struct, and getting multiple contexts
/// in nested structs.
///
/// ```
/// # use activitypub_federation::protocol::context::WithContext;
/// #[derive(serde::Serialize)]
/// struct Note {
///     content: String
/// }
/// let note = Note {
///     content: "Hello world".to_string()
/// };
/// let note_with_context = WithContext::new_default(note);
/// let serialized = serde_json::to_string(&note_with_context)?;
/// assert_eq!(serialized, r#"{"@context":[["https://www.w3.org/ns/activitystreams"]],"content":"Hello world"}"#);
/// Ok::<(), serde_json::error::Error>(())
/// ```
#[derive(Serialize, Deserialize, Debug)]
pub struct WithContext<T> {
    #[serde(rename = "@context")]
    #[serde(deserialize_with = "deserialize_one_or_many")]
    context: Vec<Value>,
    #[serde(flatten)]
    inner: T,
}

impl<T> WithContext<T> {
    /// Create a new wrapper with the default Activitypub context.
    pub fn new_default(inner: T) -> WithContext<T> {
        let context = vec![Value::from_str(DEFAULT_CONTEXT).expect("valid context")];
        WithContext::new(inner, context)
    }

    /// Create new wrapper with custom context. Use this in case you are implementing extensions.
    pub fn new(inner: T, context: Vec<Value>) -> WithContext<T> {
        WithContext { context, inner }
    }

    pub fn inner(&self) -> &T {
        &self.inner
    }
}

#[async_trait::async_trait]
impl<T> ActivityHandler for WithContext<T>
where
    T: ActivityHandler + Send,
{
    type DataType = <T as ActivityHandler>::DataType;
    type Error = <T as ActivityHandler>::Error;

    fn id(&self) -> &Url {
        self.inner.id()
    }

    fn actor(&self) -> &Url {
        self.inner.actor()
    }

    async fn receive(self, data: &RequestData<Self::DataType>) -> Result<(), Self::Error> {
        self.inner.receive(data).await
    }
}
