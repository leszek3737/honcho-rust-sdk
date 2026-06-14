//! Error types for the Honcho SDK.

use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
use httpdate::parse_http_date;
use reqwest::StatusCode;
use reqwest::header::{HeaderMap, HeaderValue};

/// Error type for all Honcho SDK operations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum HonchoError {
    /// 400 Bad Request
    #[error("Honcho API error: HTTP 400 {message}")]
    BadRequest {
        /// Error message from the API.
        message: String,
        /// Full response body if available.
        body: Option<serde_json::Value>,
    },
    /// 401 Authentication Error
    #[error("Honcho API error: HTTP 401 {message}")]
    Authentication {
        /// Error message.
        message: String,
    },
    /// 403 Permission Denied
    #[error("Honcho API error: HTTP 403 {message}")]
    PermissionDenied {
        /// Error message.
        message: String,
    },
    /// 404 Not Found
    #[error("Honcho API error: HTTP 404 {message}")]
    NotFound {
        /// Error message.
        message: String,
    },
    /// 409 Conflict
    #[error("Honcho API error: HTTP 409 {message}")]
    Conflict {
        /// Error message.
        message: String,
        /// Full response body if available.
        body: Option<serde_json::Value>,
    },
    /// 422 Unprocessable Entity
    #[error("Honcho API error: HTTP 422 {message}")]
    UnprocessableEntity {
        /// Error message.
        message: String,
        /// Full response body if available.
        body: Option<serde_json::Value>,
    },
    /// 429 Rate Limit Exceeded
    #[error("Honcho API error: HTTP 429 {message}")]
    RateLimit {
        /// Error message.
        message: String,
        /// Suggested wait time from Retry-After header.
        retry_after: Option<Duration>,
    },
    /// Unmapped or unexpected HTTP status not covered by a dedicated variant.
    ///
    /// `from_response` routes every 4xx status without a dedicated variant here
    /// (e.g. 405, 408, 413), as well as unexpected 3xx redirects and any other
    /// status that does not match a known category (e.g. `600+` from a
    /// misbehaving proxy). The `status` field preserves the original code.
    #[error("Honcho API error: HTTP {status} {message}")]
    Client {
        /// HTTP status code.
        status: u16,
        /// Error message.
        message: String,
    },
    /// 5xx Server Error
    #[error("Honcho API error: HTTP {status} {message}")]
    Server {
        /// HTTP status code.
        status: u16,
        /// Error message.
        message: String,
    },
    /// Request timed out.
    #[error("Request timed out: {message}")]
    Timeout {
        /// Error message.
        message: String,
    },
    /// Connection error.
    #[error("Connection error: {message}")]
    Connection {
        /// Error message.
        message: String,
    },
    /// HTTP transport error from reqwest.
    #[error(transparent)]
    Transport(#[from] reqwest::Error),
    /// Failed to decode response body.
    #[error("Failed to decode response at {path}: {source}")]
    Decode {
        /// JSON path where decoding failed.
        path: String,
        /// The underlying serde error.
        #[source]
        source: serde_json::Error,
    },
    /// Failed to serialize a value before sending it to the API.
    #[error("Failed to serialize {path}: {source}")]
    Serialization {
        /// Logical name of the value being serialized (e.g. a request DTO name).
        path: String,
        /// The underlying serde error.
        #[source]
        source: serde_json::Error,
    },
    /// IO error.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Configuration error.
    #[error("Configuration error: {0}")]
    Configuration(String),
    /// Validation error (e.g. duplicate inputs, invalid arguments).
    #[error("Validation error: {0}")]
    Validation(String),
    /// Partial failure in a chunked batch operation.
    ///
    /// Some chunks succeeded before an error occurred. The `messages` field
    /// contains the successfully created messages from earlier chunks, and
    /// `error` holds the underlying error that caused the failure.
    #[error("Partial failure after {sent} messages: {error}")]
    PartialFailure {
        /// Messages that were successfully created before the failure.
        messages: Vec<crate::Message>,
        /// The number of messages successfully sent.
        sent: usize,
        /// The underlying error that caused the partial failure.
        #[source]
        error: Box<HonchoError>,
    },
}

impl HonchoError {
    /// Returns a stable error code string for pattern matching.
    ///
    /// Parity with Python SDK's `error.code` field.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::BadRequest { .. } => "bad_request",
            Self::Authentication { .. } => "authentication_error",
            Self::PermissionDenied { .. } => "permission_denied",
            Self::NotFound { .. } => "not_found",
            Self::Conflict { .. } => "conflict",
            Self::UnprocessableEntity { .. } => "unprocessable_entity",
            Self::RateLimit { .. } => "rate_limit_exceeded",
            Self::Client { .. } => "client_error",
            Self::Server { .. } => "server_error",
            Self::Timeout { .. } => "timeout",
            Self::Connection { .. } => "connection_error",
            Self::Transport(_) => "transport_error",
            Self::Decode { .. } => "decode_error",
            Self::Serialization { .. } => "serialization_error",
            Self::Io(_) => "io_error",
            Self::Configuration(_) => "configuration_error",
            Self::Validation(_) => "validation_error",
            Self::PartialFailure { .. } => "partial_failure",
        }
    }

    /// Returns the HTTP status code if this error originated from an HTTP response.
    #[must_use]
    pub fn status_code(&self) -> Option<u16> {
        match self {
            Self::BadRequest { .. } => Some(400),
            Self::Authentication { .. } => Some(401),
            Self::PermissionDenied { .. } => Some(403),
            Self::NotFound { .. } => Some(404),
            Self::Conflict { .. } => Some(409),
            Self::UnprocessableEntity { .. } => Some(422),
            Self::RateLimit { .. } => Some(429),
            Self::Client { status, .. } | Self::Server { status, .. } => Some(*status),
            Self::Timeout { .. }
            | Self::Connection { .. }
            | Self::Transport(_)
            | Self::Decode { .. }
            | Self::Serialization { .. }
            | Self::Io(_)
            | Self::Configuration(_)
            | Self::Validation(_) => None,
            Self::PartialFailure { error, .. } => error.status_code(),
        }
    }

    /// Returns whether the error matches the SDK retry policy.
    ///
    /// `PartialFailure` is **never** retryable: the chunked batch already sent
    /// earlier messages, so auto-retrying the whole request would duplicate
    /// them. This is intentionally decoupled from
    /// [`retry_after`](Self::retry_after), which still surfaces any
    /// `Retry-After` hint from the underlying error so callers can decide how
    /// long to wait before a manual retry.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        if matches!(self, Self::PartialFailure { .. }) {
            return false;
        }
        matches!(self, Self::Timeout { .. } | Self::Connection { .. })
            || matches!(self.status_code(), Some(429 | 500 | 502 | 503 | 504))
    }

    /// Returns the suggested wait time for rate-limited requests.
    ///
    /// For [`PartialFailure`](Self::PartialFailure), delegates to the underlying
    /// error so callers still learn how long to wait even though the batch is
    /// not auto-retried (see [`is_retryable`](Self::is_retryable)).
    #[must_use]
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::RateLimit { retry_after, .. } => *retry_after,
            Self::PartialFailure { error, .. } => error.retry_after(),
            _ => None,
        }
    }

    /// Returns `true` if this is a partial failure with some successful messages.
    #[must_use]
    pub fn is_partial_failure(&self) -> bool {
        matches!(self, Self::PartialFailure { .. })
    }

    /// Extract the partial failure data, consuming the error.
    ///
    /// Returns `Some((messages, error))` if this is a `PartialFailure`,
    /// `None` otherwise.
    #[must_use]
    pub fn into_partial_failure(self) -> Option<(Vec<crate::Message>, Box<HonchoError>)> {
        match self {
            Self::PartialFailure {
                messages, error, ..
            } => Some((messages, error)),
            _ => None,
        }
    }

    /// Returns the human-readable error message.
    ///
    /// **Limitation (planned for a future breaking change):** for `Transport`,
    /// `Io`, `Decode`, and `Serialization` the returned string is a fixed
    /// placeholder rather than the underlying source error's detail. Inspect
    /// the source via [`Error::source`](std::error::Error::source) for the full
    /// description.
    #[must_use]
    // Each variant maps 1:1 to its `message` field today, so several arms look
    // textually identical. Kept explicit per-variant for readability; the arms
    // will diverge once the planned `message() -> Cow` change lands in a future major release.
    #[allow(clippy::match_same_arms)]
    pub fn message(&self) -> &str {
        match self {
            Self::BadRequest { message, .. } => message,
            Self::Authentication { message } => message,
            Self::PermissionDenied { message } => message,
            Self::NotFound { message } => message,
            Self::Conflict { message, .. } => message,
            Self::UnprocessableEntity { message, .. } => message,
            Self::RateLimit { message, .. } => message,
            Self::Client { message, .. } => message,
            Self::Server { message, .. } => message,
            Self::Timeout { message } => message,
            Self::Connection { message } => message,
            Self::Transport(_) => "transport error",
            Self::Io(_) => "I/O error",
            Self::Decode { .. } => "failed to decode response",
            Self::Serialization { .. } => "failed to serialize request",
            Self::Configuration(s) => s,
            Self::Validation(s) => s,
            Self::PartialFailure { error, .. } => error.message(),
        }
    }
}

/// Alias for `Result<T, HonchoError>`.
pub type Result<T> = std::result::Result<T, HonchoError>;

/// Parse an error response body, extracting message and body.
///
/// Tries to extract `detail`, `message`, or `error` fields in order (`FastAPI` convention).
#[must_use]
pub fn parse_error_body(body: &[u8]) -> (String, Option<serde_json::Value>) {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        let msg = String::from_utf8_lossy(body).into_owned();
        return (msg, None);
    };

    if let Some(obj) = value.as_object() {
        if let Some(readable) = obj.get("detail").and_then(detail_message) {
            return (readable, Some(value));
        }
        if let Some(message) = obj.get("message").and_then(|v| v.as_str()) {
            return (message.to_string(), Some(value));
        }
        if let Some(error) = obj.get("error").and_then(|v| v.as_str()) {
            return (error.to_string(), Some(value));
        }
        return (value.to_string(), Some(value));
    }

    if let Some(s) = value.as_str() {
        return (s.to_string(), Some(value));
    }

    (value.to_string(), Some(value))
}

/// Build a human-readable message from a `detail` JSON value.
///
/// `FastAPI` returns validation errors as an array of objects, each typically
/// shaped like `{"loc": [...], "msg": "...", "type": "..."}`. Without this
/// helper the whole array would be stringified into the error message. We
/// instead join the `"msg"` fields (or bare strings) with `"; "`.
///
/// Returns `None` when no readable text can be extracted, so the caller can
/// fall back to other fields or the raw JSON.
fn detail_message(detail: &serde_json::Value) -> Option<String> {
    match detail {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Array(arr) => {
            let parts: Vec<String> = arr.iter().filter_map(item_message).collect();
            if parts.is_empty() {
                None
            } else {
                Some(parts.join("; "))
            }
        }
        _ => None,
    }
}

/// Extract a single readable message from one element of a `FastAPI` `detail` array.
fn item_message(item: &serde_json::Value) -> Option<String> {
    match item {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Object(obj) => {
            obj.get("msg").and_then(|m| m.as_str()).map(str::to_owned)
        }
        _ => None,
    }
}

/// Parse a Retry-After header value.
///
/// Accepts either seconds (parsed as `f64`) or HTTP-date format
/// ([RFC 9110](https://datatracker.ietf.org/doc/html/rfc9110) §10.2.3
/// "Retry-After").
///
/// The accepted format is **looser** than RFC 9110: the standard only permits
/// non-negative integer seconds (e.g. `"120"`) or an HTTP-date, but this
/// parser uses `str::parse::<f64>()`, which additionally accepts values like
/// `"+5"`, `"1e3"`, and `"inf"`. Non-finite values (`NaN`, `±inf`) and
/// magnitudes beyond `Duration::MAX` are rejected and yield `None` rather than
/// panicking — important because the header is attacker/proxy controlled.
///
/// Returns `None` if the value cannot be parsed. Negative seconds are clamped
/// to zero (parity with Python's `max(0.0, ...)`).
#[must_use]
pub fn parse_retry_after(value: &HeaderValue, now: DateTime<Utc>) -> Option<Duration> {
    let s = value.to_str().ok()?;

    if let Ok(secs) = s.parse::<f64>() {
        // Reject non-finite values explicitly: `f64::max` ignores NaN and would
        // otherwise turn `NaN`/`-inf` into `0.0` (returning the non-NaN operand),
        // contradicting the documented "non-finite -> None" contract.
        if !secs.is_finite() {
            return None;
        }
        return Duration::try_from_secs_f64(secs.max(0.0)).ok();
    }

    let target = parse_http_date(s).ok()?;
    let now_systime: SystemTime = now.into();
    match target.duration_since(now_systime) {
        Ok(diff) => Some(diff),
        Err(_) => Some(Duration::ZERO),
    }
}

/// Construct a `HonchoError` from an HTTP response.
#[must_use]
pub fn from_response(
    status: StatusCode,
    headers: &HeaderMap,
    body: &bytes::Bytes,
    now: DateTime<Utc>,
) -> HonchoError {
    let (message, body_value) = parse_error_body(body);

    match status.as_u16() {
        400 => HonchoError::BadRequest {
            message,
            body: body_value,
        },
        401 => HonchoError::Authentication { message },
        403 => HonchoError::PermissionDenied { message },
        404 => HonchoError::NotFound { message },
        409 => HonchoError::Conflict {
            message,
            body: body_value,
        },
        422 => HonchoError::UnprocessableEntity {
            message,
            body: body_value,
        },
        429 => {
            let retry_after = headers
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| parse_retry_after(v, now));
            HonchoError::RateLimit {
                message,
                retry_after,
            }
        }
        s if s >= 500 => HonchoError::Server { status: s, message },
        s if (400..500).contains(&s) => HonchoError::Client { status: s, message },
        s if (300..400).contains(&s) => HonchoError::Client {
            status: s,
            message: format!("unexpected redirect status {s}"),
        },
        _ => HonchoError::Client {
            status: status.as_u16(),
            message: format!("unexpected response status {}", status.as_u16()),
        },
    }
}
