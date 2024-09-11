//! Remove these conversion helpers after actix-web upgrades to http 1.0

use std::str::FromStr;

pub fn header_value(v: &http02::HeaderValue) -> http::HeaderValue {
    http::HeaderValue::from_bytes(v.as_bytes()).expect("can convert http types")
}

pub fn header_map<'a, H>(m: H) -> http::HeaderMap
where
    H: IntoIterator<Item = (&'a http02::HeaderName, &'a http02::HeaderValue)>,
{
    let mut new_map = http::HeaderMap::new();
    for (n, v) in m {
        new_map.insert(
            http::HeaderName::from_lowercase(n.as_str().as_bytes())
                .expect("can convert http types"),
            header_value(v),
        );
    }
    new_map
}

pub fn method(m: &http02::Method) -> http::Method {
    http::Method::from_bytes(m.as_str().as_bytes()).expect("can convert http types")
}

pub fn uri(m: &http02::Uri) -> http::Uri {
    http::Uri::from_str(&m.to_string()).expect("can convert http types")
}
