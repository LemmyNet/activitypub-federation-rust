use axum::http::HeaderValue;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug)]
pub struct DigestPart {
    pub algorithm: String,
    pub digest: String,
}

impl DigestPart {
    pub fn try_from_header(h: &HeaderValue) -> Option<Vec<DigestPart>> {
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

pub fn verify_sha256(digests: &[DigestPart], payload: &[u8]) -> bool {
    let mut hasher = Sha256::new();

    for part in digests {
        hasher.update(payload);
        if base64::encode(hasher.finalize_reset()) != part.digest {
            return false;
        }
    }

    true
}
