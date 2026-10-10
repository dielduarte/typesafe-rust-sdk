//! The SDK error type.

use std::fmt;
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::header::HeaderMap;
use serde_json::Value;

use crate::retry::parse_retry_after;

/// Header carrying the server's request ID.
pub const REQUEST_ID_HEADER: &str = "x-typesafe-request-id";

pub(crate) fn request_id_from(headers: &HeaderMap) -> Option<String> {
    headers.get(REQUEST_ID_HEADER)?.to_str().ok().map(str::to_owned)
}

/// A specialized `Result` for SDK operations.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Everything that can go wrong when configuring the client or calling the API.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The client configuration is invalid or incomplete, for example a missing API key.
    Config(String),
    /// The request was rejected before it was sent, for example an empty question set.
    InvalidRequest(String),
    /// The server returned a non-2xx response after any retries.
    Api(Box<ApiError>),
    /// The full response did not arrive within the per-attempt timeout.
    Timeout(Duration),
    /// The request or response body failed in transit: DNS, TLS, a dropped connection.
    Connection(reqwest::Error),
    /// A 2xx response body did not match the expected shape.
    Decode(Box<DecodeError>),
}

impl Error {
    /// The HTTP status, for [`Error::Api`].
    pub fn status(&self) -> Option<StatusCode> {
        match self {
            Self::Api(e) => Some(e.status),
            _ => None,
        }
    }

    /// The server's request ID, when a response was received.
    pub fn request_id(&self) -> Option<&str> {
        match self {
            Self::Api(e) => e.request_id.as_deref(),
            Self::Decode(e) => e.request_id.as_deref(),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(msg) | Self::InvalidRequest(msg) => f.write_str(msg),
            Self::Api(e) => e.fmt(f),
            Self::Timeout(after) => write!(f, "Request timed out after {}ms.", after.as_millis()),
            Self::Connection(e) => write!(f, "Connection error: {e}"),
            Self::Decode(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Connection(e) => Some(e),
            Self::Decode(e) => Some(&e.source),
            _ => None,
        }
    }
}

/// The class of an unsuccessful HTTP status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ApiErrorKind {
    /// 400: the request is invalid.
    BadRequest,
    /// 401: authentication failed.
    Authentication,
    /// 403: access is denied.
    PermissionDenied,
    /// 404: the resource was not found.
    NotFound,
    /// 422: request validation failed.
    UnprocessableEntity,
    /// 429: the rate limit was exceeded.
    RateLimit,
    /// 5xx: the server failed to handle the request.
    InternalServer,
    /// Any other unsuccessful status.
    Other,
}

impl ApiErrorKind {
    /// Classify an HTTP status.
    pub fn from_status(status: StatusCode) -> Self {
        match status.as_u16() {
            400 => Self::BadRequest,
            401 => Self::Authentication,
            403 => Self::PermissionDenied,
            404 => Self::NotFound,
            422 => Self::UnprocessableEntity,
            429 => Self::RateLimit,
            500.. => Self::InternalServer,
            _ => Self::Other,
        }
    }
}

/// An unsuccessful HTTP response from the API.
#[derive(Debug, Clone)]
pub struct ApiError {
    /// HTTP response status.
    pub status: StatusCode,
    /// The class of `status`.
    pub kind: ApiErrorKind,
    /// Parsed JSON, the response text as a JSON string, or `None` for an empty body.
    pub body: Option<Value>,
    /// HTTP response headers.
    pub headers: HeaderMap,
    /// Request ID from `x-typesafe-request-id`.
    pub request_id: Option<String>,
    /// Server-requested retry delay from `retry-after-ms` or `Retry-After`.
    pub retry_after: Option<Duration>,
    /// A readable summary, such as `400 Unknown model: x`.
    pub message: String,
}

impl ApiError {
    pub(crate) fn new(status: StatusCode, body: Option<Value>, headers: HeaderMap) -> Self {
        Self {
            kind: ApiErrorKind::from_status(status),
            message: describe(status, body.as_ref()),
            request_id: request_id_from(&headers),
            retry_after: parse_retry_after(&headers, std::time::SystemTime::now()),
            status,
            body,
            headers,
        }
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// A 2xx response body that did not match the expected shape.
#[derive(Debug)]
pub struct DecodeError {
    /// What failed to parse.
    pub source: serde_json::Error,
    /// The response body, lossily decoded as UTF-8.
    pub body: String,
    /// Request ID from `x-typesafe-request-id`.
    pub request_id: Option<String>,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Unexpected response body: {}", self.source)?;
        if let Some(id) = &self.request_id {
            write!(f, " (request {id})")?;
        }
        Ok(())
    }
}

const MAX_RAW_BODY_IN_MESSAGE: usize = 200;

fn describe(status: StatusCode, body: Option<&Value>) -> String {
    let code = status.as_u16();
    if let Some(detail) = body.and_then(extract_message) {
        return format!("{code} {detail}");
    }
    let Some(body) = body else {
        return format!("{code} status code (no body)");
    };
    let raw = match body {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    match raw.char_indices().nth(MAX_RAW_BODY_IN_MESSAGE) {
        Some((cut, _)) => format!("{code} {}…", &raw[..cut]),
        None => format!("{code} {raw}"),
    }
}

/// Extract a message from a text, error, or validation response body.
fn extract_message(body: &Value) -> Option<String> {
    let message_of = |v: &Value| v.get("message").and_then(Value::as_str).map(str::to_owned);
    match body {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Object(o) => {
            let (error, message, detail) = (o.get("error"), o.get("message"), o.get("detail"));
            if let Some(Value::String(s)) = error {
                return Some(s.clone());
            }
            if let Some(m) = error.and_then(message_of) {
                return Some(m);
            }
            if let Some(Value::String(s)) = message {
                return Some(s.clone());
            }
            match detail? {
                Value::String(s) => Some(s.clone()),
                Value::Array(errors) => describe_validation_errors(errors),
                d => message_of(d),
            }
        }
        _ => None,
    }
}

/// Format validation errors as semicolon-separated `path: message` entries.
fn describe_validation_errors(errors: &[Value]) -> Option<String> {
    let parts: Vec<String> = errors
        .iter()
        .filter_map(|e| {
            let msg = e.get("msg")?.as_str()?;
            let loc: Vec<String> = e
                .get("loc")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|x| x.as_str() != Some("body"))
                .map(|x| match x {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect();
            Some(if loc.is_empty() { msg.to_owned() } else { format!("{}: {msg}", loc.join(".")) })
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join("; "))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn message(status: u16, body: Option<Value>) -> String {
        describe(StatusCode::from_u16(status).unwrap(), body.as_ref())
    }

    #[test]
    fn extracts_messages_from_known_shapes() {
        assert_eq!(message(400, Some(json!("plain text"))), "400 plain text");
        assert_eq!(message(400, Some(json!({"error": "Unknown model: x"}))), "400 Unknown model: x");
        assert_eq!(message(400, Some(json!({"error": {"message": "nested"}}))), "400 nested");
        assert_eq!(message(400, Some(json!({"message": "msg"}))), "400 msg");
        assert_eq!(message(400, Some(json!({"detail": "det"}))), "400 det");
        assert_eq!(message(400, Some(json!({"detail": {"message": "dm"}}))), "400 dm");
    }

    #[test]
    fn formats_validation_errors() {
        let body = json!({"detail": [
            {"loc": ["body", "questions", "q", "score", "criteria", 0], "msg": "must be a string"},
            {"loc": [], "msg": "other"},
            {"nope": true},
        ]});
        assert_eq!(message(422, Some(body)), "422 questions.q.score.criteria.0: must be a string; other");
    }

    #[test]
    fn falls_back_to_the_raw_body_truncated() {
        assert_eq!(message(500, None), "500 status code (no body)");
        assert_eq!(message(500, Some(json!({"a": 1}))), r#"500 {"a":1}"#);
        let long = "é".repeat(250);
        let msg = message(502, Some(json!({ "x": long })));
        assert!(msg.ends_with('…'));
        assert_eq!(msg.chars().count(), "502 ".len() + MAX_RAW_BODY_IN_MESSAGE + 1);
    }

    #[test]
    fn classifies_statuses() {
        let kind = |s| ApiErrorKind::from_status(StatusCode::from_u16(s).unwrap());
        assert_eq!(kind(401), ApiErrorKind::Authentication);
        assert_eq!(kind(429), ApiErrorKind::RateLimit);
        assert_eq!(kind(503), ApiErrorKind::InternalServer);
        assert_eq!(kind(418), ApiErrorKind::Other);
    }
}
