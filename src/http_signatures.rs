//! Generating keypairs, creating and verifying signatures
//!
//! Signature creation and verification is handled internally in the library. See
//! [send_activity](crate::activity_queue::send_activity) and
//! [receive_activity (actix-web)](crate::actix_web::inbox::receive_activity) /
//! [receive_activity (axum)](crate::axum::inbox::receive_activity).

use crate::{
    config::Data,
    error::{Error, Error::ActivitySignatureInvalid},
    fetch::object_id::ObjectId,
    protocol::public_key::main_key_id,
    traits::{Actor, Object},
};
use anyhow::Context;
use base64::{engine::general_purpose::STANDARD as Base64, Engine};
use bytes::Bytes;
use http::{header::HeaderName, uri::PathAndQuery, HeaderValue, Method, Uri};
use http_signature_normalization_reqwest::prelude::{Config, SignExt};
use once_cell::sync::Lazy;
use openssl::{
    hash::MessageDigest,
    pkey::{PKey, Private},
    rsa::Rsa,
    sign::{Signer, Verifier},
};
use reqwest::Request;
use reqwest_middleware::RequestBuilder;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fmt::Debug, io::ErrorKind, time::Duration};
use tracing::debug;
use url::Url;

/// A private/public key pair used for HTTP signatures
#[derive(Debug, Clone)]
pub struct Keypair {
    /// Private key in PEM format
    pub private_key: String,
    /// Public key in PEM format
    pub public_key: String,
}

impl Keypair {
    /// Helper method to turn this into an openssl private key
    #[cfg(test)]
    pub(crate) fn private_key(&self) -> Result<PKey<Private>, anyhow::Error> {
        Ok(PKey::private_key_from_pem(self.private_key.as_bytes())?)
    }
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

/// Time for which HTTP signatures are valid.
///
/// This field is optional in the standard, but required by the Rust library. It is not clear
/// what security concerns this expiration solves (if any), so we set a very high value of one hour
/// to avoid any potential problems due to wrong clocks, overloaded servers or delayed delivery.
pub(crate) const EXPIRES_AFTER: Duration = Duration::from_secs(60 * 60);

/// Creates an HTTP post request to `inbox_url`, with the given `client` and `headers`, and
/// `activity` as request body. The request is signed with `private_key` and then sent.
pub(crate) async fn sign_request(
    request_builder: RequestBuilder,
    actor_id: &Url,
    activity: Bytes,
    private_key: PKey<Private>,
    http_signature_compat: bool,
) -> Result<Request, anyhow::Error> {
    static CONFIG: Lazy<Config> = Lazy::new(|| Config::new().set_expiration(EXPIRES_AFTER));
    static CONFIG_COMPAT: Lazy<Config> = Lazy::new(|| {
        Config::new()
            .mastodon_compat()
            .set_expiration(EXPIRES_AFTER)
    });

    let key_id = main_key_id(actor_id);
    let sig_conf = match http_signature_compat {
        false => CONFIG.clone(),
        true => CONFIG_COMPAT.clone(),
    };
    request_builder
        .signature_with_digest(
            sig_conf.clone(),
            key_id,
            Sha256::new(),
            activity,
            move |signing_string| {
                let mut signer = Signer::new(MessageDigest::sha256(), &private_key)
                    .context("instantiating signer")?;
                signer
                    .update(signing_string.as_bytes())
                    .context("updating signer")?;

                Ok(Base64.encode(signer.sign_to_vec().context("sign to vec")?))
                    as Result<_, anyhow::Error>
            },
        )
        .await
}

/// Verifies the HTTP signature on an incoming federation request
/// for a given actor's public key.
///
/// Internally, this just converts the headers to a BTreeMap and passes to
/// `verify_signature_inner` for actual signature verification.
pub(crate) fn verify_signature<'a, H>(
    headers: H,
    method: &Method,
    uri: &Uri,
    public_key: &str,
) -> Result<(), Error>
where
    H: IntoIterator<Item = (&'a HeaderName, &'a HeaderValue)>,
{
    let mut header_map = BTreeMap::<String, String>::new();
    for (name, value) in headers {
        if let Ok(value) = value.to_str() {
            header_map.insert(name.to_string(), value.to_string());
        }
    }

    verify_signature_inner(header_map, method, uri, public_key)
}

/// Checks whether the given federation request has a valid signature,
/// from any actor of type A, and returns that actor if a valid signature is found.
/// This function will return an `Err` variant when no signature is found
/// or if the signature could not be verified.
pub(crate) async fn signing_actor<'a, A, H>(
    headers: H,
    method: &Method,
    uri: &Uri,
    data: &Data<<A as Object>::DataType>,
) -> Result<A, <A as Object>::Error>
where
    A: Object + Actor,
    <A as Object>::Error: From<Error> + From<anyhow::Error>,
    for<'de2> <A as Object>::Kind: Deserialize<'de2>,
    H: IntoIterator<Item = (&'a HeaderName, &'a HeaderValue)>,
{
    let mut header_map = BTreeMap::<String, String>::new();
    for (name, value) in headers {
        if let Ok(value) = value.to_str() {
            header_map.insert(name.to_string(), value.to_string());
        }
    }
    let signature = header_map
        .get("signature")
        .ok_or(Error::ActivitySignatureInvalid)?;

    let actor_id_re = regex::Regex::new("keyId=\"([^\"]+)#([^\"]+)\"").expect("regex error");
    let actor_id = match actor_id_re.captures(signature) {
        None => return Err(Error::ActivitySignatureInvalid.into()),
        Some(caps) => caps.get(1).expect("regex error").as_str(),
    };
    let actor_url = Url::parse(actor_id).map_err(|_| Error::ActivitySignatureInvalid)?;
    let actor_id: ObjectId<A> = actor_url.into();

    let actor = actor_id.dereference(data).await?;
    let public_key = actor.public_key_pem();

    verify_signature_inner(header_map, method, uri, public_key)?;

    Ok(actor)
}

/// Verifies that the signature present in the request is valid for
/// the specified actor's public key.
fn verify_signature_inner(
    header_map: BTreeMap<String, String>,
    method: &Method,
    uri: &Uri,
    public_key: &str,
) -> Result<(), Error> {
    static CONFIG: Lazy<http_signature_normalization::Config> =
        Lazy::new(|| http_signature_normalization::Config::new().set_expiration(EXPIRES_AFTER));

    let path_and_query = uri.path_and_query().map(PathAndQuery::as_str).unwrap_or("");

    let verified = CONFIG
        .begin_verify(method.as_str(), path_and_query, header_map)
        .map_err(Error::other)?
        .verify(|signature, signing_string| -> anyhow::Result<bool> {
            debug!(
                "Verifying with key {}, message {}",
                &public_key, &signing_string
            );
            let public_key = PKey::public_key_from_pem(public_key.as_bytes())?;
            let mut verifier = Verifier::new(MessageDigest::sha256(), &public_key)?;
            verifier.update(signing_string.as_bytes())?;
            Ok(verifier.verify(&Base64.decode(signature)?)?)
        })
        .map_err(Error::other)?;

    if verified {
        debug!("verified signature for {}", uri);
        Ok(())
    } else {
        Err(ActivitySignatureInvalid)
    }
}

#[derive(Clone, Debug)]
struct DigestPart {
    /// We assume that SHA256 is used which is the case with all major fediverse platforms
    #[allow(dead_code)]
    pub algorithm: String,
    /// The hashsum
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
pub(crate) fn verify_body_hash(
    digest_header: Option<&HeaderValue>,
    body: &[u8],
) -> Result<(), Error> {
    let digest = digest_header
        .and_then(DigestPart::try_from_header)
        .ok_or(Error::ActivityBodyDigestInvalid)?;
    let mut hasher = Sha256::new();

    for part in digest {
        hasher.update(body);
        if Base64.encode(hasher.finalize_reset()) != part.digest {
            return Err(Error::ActivityBodyDigestInvalid);
        }
    }

    Ok(())
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::queue::request::generate_request_headers;
    use reqwest::Client;
    use reqwest_middleware::ClientWithMiddleware;
    use std::str::FromStr;

    static ACTOR_ID: Lazy<Url> = Lazy::new(|| Url::parse("https://example.com/u/alice").unwrap());
    static INBOX_URL: Lazy<Url> =
        Lazy::new(|| Url::parse("https://example.com/u/alice/inbox").unwrap());

    #[tokio::test]
    async fn test_sign() {
        let mut headers = generate_request_headers(&INBOX_URL);
        // use hardcoded date in order to test against hardcoded signature
        headers.insert(
            "date",
            HeaderValue::from_str("Tue, 28 Mar 2023 21:03:44 GMT").unwrap(),
        );

        let request_builder = ClientWithMiddleware::from(Client::new())
            .post(INBOX_URL.to_string())
            .headers(headers);
        let request = sign_request(
            request_builder,
            &ACTOR_ID,
            "my activity".into(),
            PKey::private_key_from_pem(test_keypair().private_key.as_bytes()).unwrap(),
            // set this to prevent created/expires headers to be generated and inserted
            // automatically from current time
            true,
        )
        .await
        .unwrap();
        let signature = request
            .headers()
            .get("signature")
            .unwrap()
            .to_str()
            .unwrap();
        let expected_signature = concat!(
            "keyId=\"https://example.com/u/alice#main-key\",",
            "algorithm=\"hs2019\",",
            "headers=\"(request-target) content-type date digest host\",",
            "signature=\"BpZhHNqzd6d6jhWOxyJ0jXwWWxiKMNK7i3mrr/5mVFnH7fUpicwqw8cSYVr",
            "cwWjt0I07HW7rZFUfIdSgCoOEdvxtrccF/hTrwYgm8O6SQRHl1UfFtDR6e9EpfPieVmTjo0",
            "QVfyzLLa41rmnz/yFqqer/v0kcdED51/dGe8NCGPBbhgK6C4oh7r+XHsQZMIhh38BcfZVWN",
            "YaMqgyhFxu2f34IKnOEk6NjSaNtO+PzQUhbksTvH0Vvi6R0dtQINJFdONVBl4AwDC1INeF5",
            "uhQo/SaKHfP3UitUHdM5Pbn+LhZYDB9AaQAW5ZGD43Aw15ecwsnKi4HcjV8nBw4zehlvaQ==\""
        );
        assert_eq!(signature, expected_signature);
    }

    #[tokio::test]
    async fn test_verify() {
        let headers = generate_request_headers(&INBOX_URL);
        let request_builder = ClientWithMiddleware::from(Client::new())
            .post(INBOX_URL.to_string())
            .headers(headers);
        let request = sign_request(
            request_builder,
            &ACTOR_ID,
            "my activity".to_string().into(),
            PKey::private_key_from_pem(test_keypair().private_key.as_bytes()).unwrap(),
            false,
        )
        .await
        .unwrap();

        let valid = verify_signature(
            request.headers(),
            request.method(),
            &Uri::from_str(request.url().as_str()).unwrap(),
            &test_keypair().public_key,
        );
        println!("{:?}", &valid);
        assert!(valid.is_ok());
    }

    #[test]
    fn test_verify_body_hash_valid() {
        let digest_header =
            HeaderValue::from_static("SHA-256=lzFT+G7C2hdI5j8M+FuJg1tC+O6AGMVJhooTCKGfbKM=");
        let body = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.";
        let valid = verify_body_hash(Some(&digest_header), body.as_bytes());
        println!("{:?}", &valid);
        assert!(valid.is_ok());
    }

    #[test]
    fn test_verify_body_hash_not_valid() {
        let digest_header =
            HeaderValue::from_static("SHA-256=Z9h7DJfYWjffXw2XftmWCnpEaK/yqOHKvzCIzIaqgbU=");
        let body = "lorem ipsum";
        let invalid = verify_body_hash(Some(&digest_header), body.as_bytes());
        assert_eq!(invalid, Err(Error::ActivityBodyDigestInvalid));
    }

    pub fn test_keypair() -> Keypair {
        let rsa = Rsa::private_key_from_pem(PRIVATE_KEY.as_bytes()).unwrap();
        let pkey = PKey::from_rsa(rsa).unwrap();
        let private_key = pkey.private_key_to_pem_pkcs8().unwrap();
        let public_key = pkey.public_key_to_pem().unwrap();
        Keypair {
            private_key: String::from_utf8(private_key).unwrap(),
            public_key: String::from_utf8(public_key).unwrap(),
        }
    }

    /// Hardcoded private key so that signature doesn't change across runs
    const PRIVATE_KEY: &str = concat!(
        "-----BEGIN RSA PRIVATE KEY-----\n",
        "MIIEogIBAAKCAQEA2kZpsvWYrwM9zMQiDwo4k6/VfpK2aDTeVe9ZkcvDrrWfqt72\n",
        "QSjjtXLa8sxJlEn+/zbnZ1lG3AO/WsKs2jiOycNQHBS1ITnSZKEpdKnAoLUn4k16\n",
        "YivRmALyLedOfIrvMtQzH8a+kOQ71u2Wa3H9jpkCT5W9OneEBa3VjQp49kcrF3tm\n",
        "mrEUhfai5GJM4xrdr587y7exkBF4wObepta9opSeuBkPV4QXZPfgmjwW+oOTheVH\n",
        "6L7yjzvjW92j4/T6XKAcu0kn/aQhR8SiGtPBMyOlcW4S2eDHWf1RlqbNGb5L9Qam\n",
        "fb0WAymx0ANLUDQyXAu5zViMrd4g8mgdkg7C1wIDAQABAoIBAAHAT0Uvsguz0Frq\n",
        "0Li8+A4I4U/RQeqW6f9XtHWpl3NSYuqOPJZY2DxypHRB1Iex13x/gBHH/8jwgShR\n",
        "2x/3ev9kmsLu6f+CcdniCFQdFiRaVh/IFI0Ve7cz5tkcoiuSB2NDNcaYFwIdYqfr\n",
        "Ytz2OCn2hLQHKB9M9pLMSnDsPmMAOveY11XfhkECrWlh1bx9YPyJScnNKTblB3M+\n",
        "GhYL3xzuCxPCC9nUfqz7Y8FnZTCmePOwcRflJDTLFs6Bqkv1PZOZWzI+7akaJxfI\n",
        "SOSw3VkGegsdoGVgHobqT2tqL8vuKM1bs47PFwWjVCGEoOvcC/Ha1+INemWbh7VA\n",
        "Xa/jvxkCgYEA/+AxeMCLCmH/F696W3RpPdFL25wSYQr1auV2xRfmsT+hhpSp3yz/\n",
        "ypkazS9TbnSCm18up+jE9rJ1c9VIZrgcTeKzPURzE68RR8uOsa9o9kaUzfyvRAzb\n",
        "fmQXMvv2rmm9U7srhjpvKo1BcHpQIQYToKt0TOv7soSEY2jGNvaK6i0CgYEA2mGL\n",
        "sL36WoHF3x2DZNvknLJGjxPSMmdjjfflFRqxKeP+Sf54C4QH/1hxHe/yl/KMBTfa\n",
        "woBl05SrwTnQ7bOeR8VTmzP53JfkECT5I9h/g8vT8dkz5WQXWNDgy61Imq/UmWwm\n",
        "DHElGrkF31oy5w6+aZ58Sa5bXhBDYpkUP9+pV5MCgYAW5BCo89i8gg3XKZyxp9Vu\n",
        "cVXu/KRsSBWyjXq1oTDDNKUXrB8SVy0/C7lpF83H+OZiTf6XiOxuAYMebLtAbUIi\n",
        "+Z/9YC1HWocaPCy02rNyLNhNIUjwtpHAWeX1arMj4VPNtNXs+TdOwDpVfKvEeI2y\n",
        "9wO9ifMHgnFxj0MEUcQVtQKBgHg2Mhs8uM+RmEbVjDq9AP9w835XPuIYH6lKyIPx\n",
        "iYyxwI0i0xojt/NL0BjWuQgDsCg/MuDWpTbvJAzdsrDmqz5+1SMeXXCc/CIW+D5P\n",
        "MwJt9WGwWuzvSBrQAK6d2NWt7K335on6zp4DM8RbdqHSb+bcIza8D/ebpDxmX8s5\n",
        "Z5KZAoGAX8u+63w1uy1FLhf48SqmjOqkAjdUZCWEmaim69koAOdTIBSSDOnAqzGu\n",
        "wIVdLLzI6xTgbYmfErCwpU2v8MfUWr0BDzjQ9G6c5rhcS1BkfxbeAsC42XaVIgCk\n",
        "2sMNMqi6f96jbp4IQI70BpecsnBAUa+VoT57bZRvy0lW26w9tYI=\n",
        "-----END RSA PRIVATE KEY-----\n"
    );
}
