use serde::{Deserialize, Serialize};
use url::Url;

/// Public key of actors which is used for HTTP signatures. This needs to be federated in the
/// `public_key` field of all actors.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicKey {
    pub(crate) id: String,
    pub(crate) owner: Url,
    pub public_key_pem: String,
}

impl PublicKey {
    pub fn new(owner: Url, public_key_pem: String) -> Self {
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
