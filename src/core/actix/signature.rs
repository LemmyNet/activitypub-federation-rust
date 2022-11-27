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
