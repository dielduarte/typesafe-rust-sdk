//! Request logging through `tracing` (behind the `tracing` feature) and header redaction.

use reqwest::header::{HeaderMap, HeaderName};

/// Emit an event at `info` with the `tracing` feature; otherwise compile to nothing.
macro_rules! info {
    ($($arg:tt)*) => {{
        #[cfg(feature = "tracing")]
        ::tracing::info!($($arg)*);
        #[cfg(not(feature = "tracing"))]
        if false {
            let _ = ::std::format!($($arg)*);
        }
    }};
}

/// Emit an event at `debug` with the `tracing` feature; otherwise compile to nothing.
macro_rules! debug {
    ($($arg:tt)*) => {{
        #[cfg(feature = "tracing")]
        ::tracing::debug!($($arg)*);
        #[cfg(not(feature = "tracing"))]
        if false {
            let _ = ::std::format!($($arg)*);
        }
    }};
}

pub(crate) use {debug, info};

/// Credential headers that keep a key suffix for identification.
const KEY_HEADERS: [&str; 3] = ["authorization", "proxy-authorization", "x-api-key"];

/// Headers whose values are redacted in full.
const OPAQUE_HEADERS: [&str; 2] = ["cookie", "set-cookie"];

/// Mask a key, keeping its scheme and the last four characters of secrets longer than eight.
fn redact_key(value: &str) -> String {
    let (scheme, secret) = match value.split_once(char::is_whitespace) {
        Some((scheme, secret)) => (Some(scheme), secret.trim_start()),
        None => (None, value),
    };
    let chars: Vec<char> = secret.chars().collect();
    let tail: String = if chars.len() > 8 { chars[chars.len() - 4..].iter().collect() } else { String::new() };
    match scheme {
        Some(scheme) => format!("{scheme} ***{tail}"),
        None => format!("***{tail}"),
    }
}

fn redact(name: &HeaderName, value: &str) -> String {
    let name = name.as_str();
    if KEY_HEADERS.contains(&name) {
        redact_key(value)
    } else if OPAQUE_HEADERS.contains(&name) {
        "***".into()
    } else {
        value.into()
    }
}

/// Header names and values with known credentials redacted, for logs.
pub(crate) fn redact_headers(headers: &HeaderMap) -> Vec<(String, String)> {
    headers.iter().map(|(name, value)| (name.to_string(), redact(name, value.to_str().unwrap_or("<binary>")))).collect()
}

#[cfg(test)]
mod tests {
    use reqwest::header::HeaderValue;

    use super::*;

    #[test]
    fn redacts_credentials_and_keeps_other_headers() {
        let headers: HeaderMap = [
            ("authorization", "Bearer sk_live_1234567890"),
            ("x-api-key", "short"),
            ("cookie", "session=abc"),
            ("accept", "application/json"),
        ]
        .into_iter()
        .map(|(k, v)| (HeaderName::from_static(k), HeaderValue::from_static(v)))
        .collect();

        assert_eq!(
            redact_headers(&headers),
            [
                ("authorization", "Bearer ***7890"),
                ("x-api-key", "***"),
                ("cookie", "***"),
                ("accept", "application/json"),
            ]
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
        );
    }
}
