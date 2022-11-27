use anyhow::anyhow;
use http::{HeaderMap, Uri};
use http_signature_normalization::Config;
use openssl::{hash::MessageDigest, pkey::PKey, sign::Verifier};
use std::collections::BTreeMap;
use tracing::debug;

/// Verifies the HTTP signature on an incoming inbox request.
pub fn verify_signature(
    headers: &HeaderMap,
    uri: Uri,
    public_key: &str,
) -> Result<(), anyhow::Error> {
    let config = Config::default();
    let mut header_map = BTreeMap::new();
    for (name, value) in headers {
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
        debug!("verified signature for {}", uri);
        Ok(())
    } else {
        Err(anyhow!("Invalid signature on request: {}", uri))
    }
}
