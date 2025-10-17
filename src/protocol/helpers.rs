//! Serde deserialization functions which help to receive differently shaped data

use serde::{Deserialize, Deserializer};

/// Deserialize JSON single value or array into Vec.
///
/// Useful if your application can handle multiple values for a field, but another federated
/// platform only sends a single one.
///
/// ```
/// # use activitypub_federation::protocol::helpers::deserialize_one_or_many;
/// # use url::Url;
/// #[derive(serde::Deserialize)]
/// struct Note {
///     #[serde(deserialize_with = "deserialize_one_or_many")]
///     to: Vec<Url>
/// }
///
/// let single: Note = serde_json::from_str(r#"{"to": "https://example.com/u/alice" }"#)?;
/// assert_eq!(single.to.len(), 1);
///
/// let multiple: Note = serde_json::from_str(
/// r#"{"to": [
///      "https://example.com/u/alice",
///      "https://lemmy.ml/u/bob"
/// ]}"#)?;
/// assert_eq!(multiple.to.len(), 2);
/// Ok::<(), anyhow::Error>(())
pub fn deserialize_one_or_many<'de, T, D>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany<T> {
        One(T),
        Many(Vec<T>),
    }

    let result: OneOrMany<T> = Deserialize::deserialize(deserializer)?;
    Ok(match result {
        OneOrMany::Many(list) => list,
        OneOrMany::One(value) => vec![value],
    })
}

/// Deserialize JSON single value or single element array into single value.
///
/// Useful if your application can only handle a single value for a field, but another federated
/// platform sends single value wrapped in array. Fails if array contains multiple items.
///
/// ```
/// # use activitypub_federation::protocol::helpers::deserialize_one;
/// # use url::Url;
/// #[derive(serde::Deserialize)]
/// struct Note {
///     #[serde(deserialize_with = "deserialize_one")]
///     to: [Url; 1]
/// }
///
/// let note = serde_json::from_str::<Note>(r#"{"to": ["https://example.com/u/alice"] }"#);
/// assert!(note.is_ok());
pub fn deserialize_one<'de, T, D>(deserializer: D) -> Result<[T; 1], D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum MaybeArray<T> {
        Simple(T),
        Array([T; 1]),
    }

    let result: MaybeArray<T> = Deserialize::deserialize(deserializer)?;
    Ok(match result {
        MaybeArray::Simple(value) => [value],
        MaybeArray::Array([value]) => [value],
    })
}

/// Attempts to deserialize item, in case of error falls back to the type's default value.
///
/// Useful for optional fields which are sent with a different type from another platform,
/// eg object instead of array. Should always be used together with `#[serde(default)]`, so
/// that a mssing value doesn't cause an error.
///
/// ```
/// # use activitypub_federation::protocol::helpers::deserialize_skip_error;
/// # use url::Url;
/// #[derive(serde::Deserialize)]
/// struct Note {
///     content: String,
///     #[serde(deserialize_with = "deserialize_skip_error", default)]
///     source: Option<String>
/// }
///
/// let note = serde_json::from_str::<Note>(
/// r#"{
///     "content": "How are you?",
///     "source": {
///         "content": "How are you?",
///         "mediaType": "text/markdown"
///     }
/// }"#);
/// assert_eq!(note.unwrap().source, None);
/// # Ok::<(), anyhow::Error>(())
pub fn deserialize_skip_error<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    T: Deserialize<'de> + Default,
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    let inner = T::deserialize(value).unwrap_or_default();
    Ok(inner)
}

/// Deserialize either single value or last item from an array into an optional field.
pub fn deserialize_last<'de, T, D>(deserializer: D) -> Result<Option<T>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum MaybeArray<T> {
        Simple(T),
        Array(Vec<T>),
        None,
    }

    let result = Deserialize::deserialize(deserializer)?;
    Ok(match result {
        MaybeArray::Simple(value) => Some(value),
        MaybeArray::Array(value) => value.into_iter().last(),
        MaybeArray::None => None,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn deserialize_one_multiple_values() {
        use crate::protocol::helpers::deserialize_one;
        use url::Url;
        #[derive(serde::Deserialize)]
        struct Note {
            #[serde(deserialize_with = "deserialize_one")]
            _to: [Url; 1],
        }

        let note = serde_json::from_str::<Note>(
            r#"{"_to": ["https://example.com/u/alice", "https://example.com/u/bob"] }"#,
        );
        assert!(note.is_err());
    }
}
