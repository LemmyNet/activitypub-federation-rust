//! Tombstone is used to serve deleted objects

use crate::kinds::object::TombstoneType;
use serde::{Deserialize, Serialize};
use url::Url;

/// Represents a local object that was deleted
///
/// <https://www.w3.org/TR/activitystreams-vocabulary/#dfn-tombstone>
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tombstone {
    /// Id of the deleted object
    pub id: Url,
    #[serde(rename = "type")]
    pub(crate) kind: TombstoneType,
}

impl Tombstone {
    /// Create a new tombstone for the given object id
    pub fn new(id: Url) -> Tombstone {
        Tombstone {
            id,
            kind: TombstoneType::Tombstone,
        }
    }
}
