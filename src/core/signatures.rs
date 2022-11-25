use http_signature_normalization_reqwest::prelude::{Config, SignExt};
use once_cell::sync::OnceCell;
use openssl::{hash::MessageDigest, pkey::PKey, rsa::Rsa, sign::Signer};
use reqwest::Request;
use reqwest_middleware::RequestBuilder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Error, ErrorKind};
use url::Url;

static HTTP_SIG_CONFIG: OnceCell<Config> = OnceCell::new();

/// A private/public key pair used for HTTP signatures
#[derive(Debug, Clone)]
pub struct Keypair {
    pub private_key: String,
    pub public_key: String,
}

/// Generate the asymmetric keypair for ActivityPub HTTP signatures.
pub fn generate_actor_keypair() -> Result<Keypair, Error> {
    let rsa = Rsa::generate(2048)?;
    let pkey = PKey::from_rsa(rsa)?;
    let public_key = pkey.public_key_to_pem()?;
    let private_key = pkey.private_key_to_pem_pkcs8()?;
    let key_to_string = |key| match String::from_utf8(key) {
        Ok(s) => Ok(s),
        Err(e) => Err(Error::new(
            ErrorKind::Other,
            format!("Failed converting key to string: {}", e),
        )),
    };
    Ok(Keypair {
        private_key: key_to_string(private_key)?,
        public_key: key_to_string(public_key)?,
    })
}

/// Creates an HTTP post request to `inbox_url`, with the given `client` and `headers`, and
/// `activity` as request body. The request is signed with `private_key` and then sent.
pub(crate) async fn sign_request(
    request_builder: RequestBuilder,
    activity: String,
    public_key: PublicKey,
    private_key: String,
    http_signature_compat: bool,
) -> Result<Request, anyhow::Error> {
    let sig_conf = HTTP_SIG_CONFIG.get_or_init(|| {
        let c = Config::new();
        if http_signature_compat {
            c.mastodon_compat()
        } else {
            c
        }
    });
    request_builder
        .signature_with_digest(
            sig_conf.clone(),
            public_key.id,
            Sha256::new(),
            activity,
            move |signing_string| {
                let private_key = PKey::private_key_from_pem(private_key.as_bytes())?;
                let mut signer = Signer::new(MessageDigest::sha256(), &private_key)?;
                signer.update(signing_string.as_bytes())?;

                Ok(base64::encode(signer.sign_to_vec()?)) as Result<_, anyhow::Error>
            },
        )
        .await
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicKey {
    pub(crate) id: String,
    pub(crate) owner: Url,
    pub public_key_pem: String,
}

impl PublicKey {
    /// Create public key with default id, for actors that only have a single keypair
    pub fn new_main_key(owner: Url, public_key_pem: String) -> Self {
        let key_id = format!("{}#main-key", &owner);
        PublicKey::new(key_id, owner, public_key_pem)
    }

    /// Create public key with custom key id. Use this method if there are multiple keypairs per actor
    pub fn new(id: String, owner: Url, public_key_pem: String) -> Self {
        PublicKey {
            id,
            owner,
            public_key_pem,
        }
    }
}

#[cfg(feature = "actix")]
pub mod actix {
    use actix_web::HttpRequest;
    use anyhow::anyhow;
    use http_signature_normalization_actix::Config as ConfigActix;
    use once_cell::sync::Lazy;
    use openssl::{hash::MessageDigest, pkey::PKey, sign::Verifier};
    use tracing::debug;

    static CONFIG2: Lazy<ConfigActix> = Lazy::new(ConfigActix::new);

    /// Verifies the HTTP signature on an incoming inbox request.
    pub fn verify_signature(request: &HttpRequest, public_key: &str) -> Result<(), anyhow::Error> {
        let verified = CONFIG2
            .begin_verify(
                request.method(),
                request.uri().path_and_query(),
                request.headers().clone(),
            )?
            .verify(|signature, signing_string| -> anyhow::Result<bool> {
                debug!(
                    "Verifying with key {}, message {}",
                    &public_key, &signing_string
                );
                let public_key = PKey::public_key_from_pem(public_key.as_bytes())?;
                let mut verifier = Verifier::new(MessageDigest::sha256(), &public_key)?;
                verifier.update(signing_string.as_bytes())?;
                Ok(verifier.verify(&base64::decode(signature)?)?)
            })?;

        if verified {
            debug!("verified signature for {}", &request.uri());
            Ok(())
        } else {
            Err(anyhow!("Invalid signature on request: {}", &request.uri()))
        }
    }
}

#[cfg(feature = "axum")]
pub mod axum {
    use anyhow::anyhow;
    use axum::http::Request;
    use http_signature_normalization::Config;
    use openssl::{hash::MessageDigest, pkey::PKey, sign::Verifier};
    use std::collections::BTreeMap;
    use tracing::debug;

    /// Verifies the HTTP signature on an incoming inbox request.
    pub fn verify_signature<B>(
        request: &Request<B>,
        public_key: &str,
    ) -> Result<(), anyhow::Error> {
        let config = Config::default();
        let mut header_map = BTreeMap::new();
        for (name, value) in request.headers() {
            if let Ok(value) = value.to_str() {
                header_map.insert(name.to_string(), value.to_string());
            }
        }

        let verified = config
            .begin_verify("GET", "/foo?bar=baz", header_map)?
            .verify(|signature, signing_string| -> anyhow::Result<bool> {
                debug!(
                    "Verifying with key {}, message {}",
                    &public_key, &signing_string
                );
                let public_key = PKey::public_key_from_pem(public_key.as_bytes())?;
                let mut verifier = Verifier::new(MessageDigest::sha256(), &public_key)?;
                verifier.update(signing_string.as_bytes())?;
                Ok(verifier.verify(&base64::decode(signature)?)?)
            })?;

        if verified {
            debug!("verified signature for {}", &request.uri());
            Ok(())
        } else {
            Err(anyhow!("Invalid signature on request: {}", &request.uri()))
        }
    }
}
