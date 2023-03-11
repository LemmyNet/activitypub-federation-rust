//! Struct which is used to federate actor key for HTTP signatures

use serde::{Deserialize, Serialize};
use url::Url;

/// Public key of actors which is used for HTTP signatures.
///
/// This needs to be federated in the `public_key` field of all actors.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicKey {
    /// Id of this private key.
    pub id: String,
    /// ID of the actor that this public key belongs to
    pub owner: Url,
    /// The actual public key in PEM format
    pub public_key_pem: String,
}

impl PublicKey {
    /// Create a new [PublicKey] struct for the `owner` with `public_key_pem`.
    ///
    /// It uses an standard key id of `{actor_id}#main-key`
    pub(crate) fn new(owner: Url, public_key_pem: String) -> Self {
        let id = main_key_id(&owner);
        PublicKey {
            id,
            owner,
            public_key_pem,
        }
    }
}

pub(crate) fn main_key_id(owner: &Url) -> String {
    format!("{}#main-key", &owner)
}
