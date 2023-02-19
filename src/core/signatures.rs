use crate::{utils::header_to_map, Error, Error::ActivitySignatureInvalid};
use anyhow::anyhow;
use http::{header::HeaderName, uri::PathAndQuery, HeaderValue, Method, Uri};
use http_signature_normalization_reqwest::prelude::{Config, SignExt};
use once_cell::sync::{Lazy, OnceCell};
use openssl::{
    hash::MessageDigest,
    pkey::PKey,
    rsa::Rsa,
    sign::{Signer, Verifier},
};
use reqwest::Request;
use reqwest_middleware::RequestBuilder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::ErrorKind;
use tracing::debug;
use url::Url;

static HTTP_SIG_CONFIG: OnceCell<Config> = OnceCell::new();

/// A private/public key pair used for HTTP signatures
#[derive(Debug, Clone)]
pub struct Keypair {
    pub private_key: String,
    pub public_key: String,
}

/// Generate a random asymmetric keypair for ActivityPub HTTP signatures.
pub fn generate_actor_keypair() -> Result<Keypair, std::io::Error> {
    let rsa = Rsa::generate(2048)?;
    let pkey = PKey::from_rsa(rsa)?;
    let public_key = pkey.public_key_to_pem()?;
    let private_key = pkey.private_key_to_pem_pkcs8()?;
    let key_to_string = |key| match String::from_utf8(key) {
        Ok(s) => Ok(s),
        Err(e) => Err(std::io::Error::new(
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

static CONFIG2: Lazy<http_signature_normalization::Config> =
    Lazy::new(http_signature_normalization::Config::new);

/// Verifies the HTTP signature on an incoming inbox request.
pub fn verify_signature<'a, H>(
    headers: H,
    method: &Method,
    uri: &Uri,
    public_key: &str,
) -> Result<(), Error>
where
    H: IntoIterator<Item = (&'a HeaderName, &'a HeaderValue)>,
{
    let headers = header_to_map(headers);
    let path_and_query = uri.path_and_query().map(PathAndQuery::as_str).unwrap_or("");

    let verified = CONFIG2
        .begin_verify(method.as_str(), path_and_query, headers)
        .map_err(Error::conv)?
        .verify(|signature, signing_string| -> anyhow::Result<bool> {
            debug!(
                "Verifying with key {}, message {}",
                &public_key, &signing_string
            );
            let public_key = PKey::public_key_from_pem(public_key.as_bytes())?;
            let mut verifier = Verifier::new(MessageDigest::sha256(), &public_key)?;
            verifier.update(signing_string.as_bytes())?;
            Ok(verifier.verify(&base64::decode(signature)?)?)
        })
        .map_err(Error::conv)?;

    if verified {
        debug!("verified signature for {}", uri);
        Ok(())
    } else {
        Err(ActivitySignatureInvalid)
    }
}

#[derive(Clone, Debug)]
struct DigestPart {
    pub algorithm: String,
    pub digest: String,
}

impl DigestPart {
    fn try_from_header(h: &HeaderValue) -> Option<Vec<DigestPart>> {
        let h = h.to_str().ok()?.split(';').next()?;
        let v: Vec<_> = h
            .split(',')
            .filter_map(|p| {
                let mut iter = p.splitn(2, '=');
                iter.next()
                    .and_then(|alg| iter.next().map(|value| (alg, value)))
            })
            .map(|(alg, value)| DigestPart {
                algorithm: alg.to_owned(),
                digest: value.to_owned(),
            })
            .collect();

        if v.is_empty() {
            None
        } else {
            Some(v)
        }
    }
}

/// Verify body of an inbox request against the hash provided in `Digest` header.
pub(crate) fn verify_inbox_hash(
    digest_header: Option<&HeaderValue>,
    body: &[u8],
) -> Result<(), crate::Error> {
    let digests = DigestPart::try_from_header(digest_header.unwrap()).unwrap();
    let mut hasher = Sha256::new();

    for part in digests {
        hasher.update(body);
        if base64::encode(hasher.finalize_reset()) != part.digest {
            return Err(crate::Error::ActivityBodyDigestInvalid);
        }
    }

    Ok(())
}
