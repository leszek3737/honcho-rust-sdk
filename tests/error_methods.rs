#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

//! Tests for `HonchoError` accessor methods: `code()`, `status_code()`,
//! `message()`, `is_retryable()`, `retry_after()`, `is_partial_failure()`,
//! `into_partial_failure()`, and the `from_response` factory.
//!
//! Coverage ownership split with `tests/error_mapping.rs` (A2):
//! - A3 (this file) owns `code()` for the **constructed-directly** variants:
//!   `Timeout`, `Connection`, `Transport`, `Decode`, `Io`, `Configuration`,
//!   `Validation`, `PartialFailure`.
//! - A2 owns `code()` for the **HTTP-status-mapped** variants reached via
//!   `from_response`: `BadRequest`, `Authentication`, `PermissionDenied`,
//!   `NotFound`, `Conflict`, `UnprocessableEntity`, `RateLimit`, `Client`,
//!   `Server`.

use std::time::Duration;

use honcho_ai::error::{HonchoError, from_response};

// ─── Fixtures ──────────────────────────────────────────────────────────

/// `BadRequest` — a 4xx variant carrying an optional `body`. Named to
/// distinguish from `not_found_error` which has no `body` field.
fn bad_request_error() -> HonchoError {
    HonchoError::BadRequest {
        message: "bad input".into(),
        body: None,
    }
}

/// `NotFound` — a 4xx variant with no `body` field. Named to distinguish
/// from `bad_request_error` which carries `body`.
fn not_found_error() -> HonchoError {
    HonchoError::NotFound {
        message: "not found".into(),
    }
}

/// `PartialFailure` wrapping a 500 `Server` error. `sent` is consistent with
/// `messages.len()` (item 11: the old `sent: 5` with empty Vec was
/// contradictory and `sent` was never asserted).
///
/// Note: a non-empty `Vec<Message>` cannot be constructed from this external
/// test file because `Message::from_raw` is `pub(crate)`. The
/// `into_partial_failure_extracts` test asserts the strongest contract
/// available given that API boundary.
fn partial_failure_server_error() -> HonchoError {
    HonchoError::PartialFailure {
        messages: vec![],
        sent: 0,
        error: Box::new(HonchoError::Server {
            status: 500,
            message: "server boom".into(),
        }),
    }
}

/// `PartialFailure` wrapping a `Timeout` — proves `is_retryable` / `status_code`
/// delegate consistently regardless of the inner variant, and that the inner
/// variant's retryability does NOT leak through `PartialFailure`.
fn partial_failure_timeout_error() -> HonchoError {
    HonchoError::PartialFailure {
        messages: vec![],
        sent: 0,
        error: Box::new(HonchoError::Timeout {
            message: "inner timeout".into(),
        }),
    }
}

/// `PartialFailure` wrapping a 503 `Server` error — the critical case where
/// the inner 503 is normally retryable, but the AGREED SEMANTICS contract
/// mandates `is_retryable() == false` for *any* `PartialFailure`.
fn partial_failure_server_503_error() -> HonchoError {
    HonchoError::PartialFailure {
        messages: vec![],
        sent: 0,
        error: Box::new(HonchoError::Server {
            status: 503,
            message: "unavailable".into(),
        }),
    }
}

/// `PartialFailure` wrapping a `RateLimit` carrying a `retry_after`. Proves
/// `retry_after()` delegation transitively through `PartialFailure`.
fn partial_failure_rate_limit_error() -> HonchoError {
    HonchoError::PartialFailure {
        messages: vec![],
        sent: 0,
        error: Box::new(rate_limit_with_retry_after()),
    }
}

fn rate_limit_with_retry_after() -> HonchoError {
    HonchoError::RateLimit {
        message: "slow down".into(),
        retry_after: Some(Duration::from_secs(30)),
    }
}

fn rate_limit_without_retry_after() -> HonchoError {
    HonchoError::RateLimit {
        message: "slow down".into(),
        retry_after: None,
    }
}

fn io_error() -> HonchoError {
    let io = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
    HonchoError::Io(io)
}

/// Constructs a `Decode` error from a real `serde_json::Error`.
fn decode_error() -> HonchoError {
    let source = serde_json::from_str::<serde_json::Value>("not json")
        .expect_err("malformed JSON must fail");
    HonchoError::Decode {
        path: "$.data".into(),
        source,
    }
}

/// Constructs a `Transport` error from a real `reqwest::Error`.
///
/// `reqwest::Error` has no public constructor; we obtain one synchronously by
/// building a request with a malformed URL. `RequestBuilder::build()` defers
/// `IntoUrl::into_url()` and returns the parse error without any async runtime.
fn transport_error() -> HonchoError {
    let err = reqwest::Client::new()
        .get("http://[")
        .build()
        .expect_err("malformed URL must fail to build");
    HonchoError::Transport(err)
}

// ─── message() ─────────────────────────────────────────────────────────
//
// Table-driven. `Transport` / `Io` / `Decode` return static placeholder
// strings (a future major release is tracked to switch `message()` to `Cow` and surface the
// underlying error detail; until then we assert the current placeholders).

#[test]
fn message_table() {
    let cases: Vec<(&str, HonchoError, &str)> = vec![
        ("BadRequest", bad_request_error(), "bad input"),
        (
            "Authentication",
            HonchoError::Authentication {
                message: "no key".into(),
            },
            "no key",
        ),
        (
            "PermissionDenied",
            HonchoError::PermissionDenied {
                message: "forbidden".into(),
            },
            "forbidden",
        ),
        ("NotFound", not_found_error(), "not found"),
        (
            "Conflict",
            HonchoError::Conflict {
                message: "dup".into(),
                body: None,
            },
            "dup",
        ),
        (
            "UnprocessableEntity",
            HonchoError::UnprocessableEntity {
                message: "u".into(),
                body: None,
            },
            "u",
        ),
        ("RateLimit", rate_limit_without_retry_after(), "slow down"),
        (
            "Client",
            HonchoError::Client {
                status: 418,
                message: "teapot".into(),
            },
            "teapot",
        ),
        (
            "Server",
            HonchoError::Server {
                status: 503,
                message: "down".into(),
            },
            "down",
        ),
        (
            "Timeout",
            HonchoError::Timeout {
                message: "timed out".into(),
            },
            "timed out",
        ),
        (
            "Connection",
            HonchoError::Connection {
                message: "refused".into(),
            },
            "refused",
        ),
        (
            "Configuration",
            HonchoError::Configuration("bad config".into()),
            "bad config",
        ),
        (
            "Validation",
            HonchoError::Validation("bad val".into()),
            "bad val",
        ),
        // Static placeholders — documents the `Cow` limitation.
        ("Transport", transport_error(), "transport error"),
        ("Io", io_error(), "I/O error"),
        ("Decode", decode_error(), "failed to decode response"),
        // PartialFailure delegates to inner.
        (
            "PartialFailure(Server)",
            partial_failure_server_error(),
            "server boom",
        ),
    ];

    for (label, err, expected) in cases {
        assert_eq!(err.message(), expected, "message() mismatch for {label}");
    }
}

#[test]
fn message_partial_failure_delegates_through_chain() {
    // PartialFailure(PartialFailure(...)) — delegation is recursive.
    let inner = partial_failure_server_error();
    let outer = HonchoError::PartialFailure {
        messages: vec![],
        sent: 0,
        error: Box::new(inner),
    };
    assert_eq!(outer.message(), "server boom");
}

// ─── status_code() ─────────────────────────────────────────────────────
//
// Table-driven. Covers the previously-missing hardcoded mappings (401/403/
// 409/422), the `None` returns for non-HTTP variants, and `PartialFailure`
// delegation.

#[test]
fn status_code_table() {
    let cases: Vec<(&str, HonchoError, Option<u16>)> = vec![
        // Hardcoded HTTP-status mappings.
        ("BadRequest", bad_request_error(), Some(400)),
        (
            "Authentication",
            HonchoError::Authentication {
                message: "a".into(),
            },
            Some(401),
        ),
        (
            "PermissionDenied",
            HonchoError::PermissionDenied {
                message: "p".into(),
            },
            Some(403),
        ),
        ("NotFound", not_found_error(), Some(404)),
        (
            "Conflict",
            HonchoError::Conflict {
                message: "c".into(),
                body: None,
            },
            Some(409),
        ),
        (
            "UnprocessableEntity",
            HonchoError::UnprocessableEntity {
                message: "u".into(),
                body: None,
            },
            Some(422),
        ),
        ("RateLimit", rate_limit_without_retry_after(), Some(429)),
        // Client/Server echo the embedded status.
        (
            "Client{418}",
            HonchoError::Client {
                status: 418,
                message: "t".into(),
            },
            Some(418),
        ),
        (
            "Server{503}",
            HonchoError::Server {
                status: 503,
                message: "s".into(),
            },
            Some(503),
        ),
        // Non-HTTP variants → None.
        (
            "Timeout",
            HonchoError::Timeout {
                message: "t".into(),
            },
            None,
        ),
        (
            "Connection",
            HonchoError::Connection {
                message: "c".into(),
            },
            None,
        ),
        ("Transport", transport_error(), None),
        ("Decode", decode_error(), None),
        ("Io", io_error(), None),
        (
            "Configuration",
            HonchoError::Configuration("c".into()),
            None,
        ),
        ("Validation", HonchoError::Validation("v".into()), None),
        // PartialFailure delegates to the inner error.
        (
            "PartialFailure(Server500)",
            partial_failure_server_error(),
            Some(500),
        ),
        (
            "PartialFailure(Timeout)",
            partial_failure_timeout_error(),
            None,
        ),
        (
            "PartialFailure(Server503)",
            partial_failure_server_503_error(),
            Some(503),
        ),
    ];

    for (label, err, expected) in cases {
        assert_eq!(
            err.status_code(),
            expected,
            "status_code() mismatch for {label}"
        );
    }
}

// ─── code() — constructed-directly variants (A3 ownership) ─────────────
//
// A3 owns these 8 variants. A2 (`tests/error_mapping.rs`) covers the 9
// HTTP-status-mapped variants reached via `from_response`.

#[test]
fn code_constructed_directly_variants() {
    let cases: Vec<(&str, HonchoError, &str)> = vec![
        (
            "Timeout",
            HonchoError::Timeout {
                message: "t".into(),
            },
            "timeout",
        ),
        (
            "Connection",
            HonchoError::Connection {
                message: "c".into(),
            },
            "connection_error",
        ),
        ("Transport", transport_error(), "transport_error"),
        ("Decode", decode_error(), "decode_error"),
        ("Io", io_error(), "io_error"),
        (
            "Configuration",
            HonchoError::Configuration("c".into()),
            "configuration_error",
        ),
        (
            "Validation",
            HonchoError::Validation("v".into()),
            "validation_error",
        ),
        (
            "PartialFailure",
            partial_failure_server_error(),
            "partial_failure",
        ),
    ];

    for (label, err, expected) in cases {
        assert_eq!(err.code(), expected, "code() mismatch for {label}");
    }
}

// ─── is_retryable() — full variant matrix (CRITICAL) ───────────────────
//
// Table-driven. Asserts `is_retryable()` for EVERY `HonchoError` variant.
// The retry policy: `Timeout` / `Connection` / HTTP 429|500|502|503|504 are
// retryable; all other variants are not. `PartialFailure` is NEVER retryable
// (AGREED SEMANTICS), even when the inner error (e.g. `Server{503}`) would be
// retryable on its own — a partial batch must not be blindly retried.
//
// The case table is split into two helpers (retryable / non-retryable) so each
// stays under the clippy line budget; together they are the authoritative
// coverage record for `is_retryable`.

#[allow(clippy::type_complexity)]
fn is_retryable_true_cases() -> Vec<(&'static str, HonchoError)> {
    vec![
        (
            "Timeout",
            HonchoError::Timeout {
                message: "t".into(),
            },
        ),
        (
            "Connection",
            HonchoError::Connection {
                message: "c".into(),
            },
        ),
        ("RateLimit{429}", rate_limit_without_retry_after()),
        (
            "Server{500}",
            HonchoError::Server {
                status: 500,
                message: "s".into(),
            },
        ),
        (
            "Server{502}",
            HonchoError::Server {
                status: 502,
                message: "s".into(),
            },
        ),
        (
            "Server{503}",
            HonchoError::Server {
                status: 503,
                message: "s".into(),
            },
        ),
        (
            "Server{504}",
            HonchoError::Server {
                status: 504,
                message: "s".into(),
            },
        ),
    ]
}

#[allow(clippy::type_complexity)]
fn is_retryable_false_cases() -> Vec<(&'static str, HonchoError)> {
    vec![
        (
            "Server{501}",
            HonchoError::Server {
                status: 501,
                message: "s".into(),
            },
        ),
        ("BadRequest{400}", bad_request_error()),
        (
            "Authentication{401}",
            HonchoError::Authentication {
                message: "a".into(),
            },
        ),
        (
            "PermissionDenied{403}",
            HonchoError::PermissionDenied {
                message: "p".into(),
            },
        ),
        ("NotFound{404}", not_found_error()),
        (
            "Conflict{409}",
            HonchoError::Conflict {
                message: "c".into(),
                body: None,
            },
        ),
        (
            "UnprocessableEntity{422}",
            HonchoError::UnprocessableEntity {
                message: "u".into(),
                body: None,
            },
        ),
        (
            "Client{405}",
            HonchoError::Client {
                status: 405,
                message: "m".into(),
            },
        ),
        ("Transport", transport_error()),
        ("Decode", decode_error()),
        ("Io", io_error()),
        ("Configuration", HonchoError::Configuration("c".into())),
        ("Validation", HonchoError::Validation("v".into())),
        // PartialFailure: ALWAYS non-retryable (AGREED SEMANTICS).
        // Inner 503 would be retryable on its own — the wrapper must override.
        (
            "PartialFailure(Server503)",
            partial_failure_server_503_error(),
        ),
        ("PartialFailure(Timeout)", partial_failure_timeout_error()),
    ]
}

#[test]
fn is_retryable_full_matrix() {
    for (label, err) in is_retryable_true_cases() {
        assert!(err.is_retryable(), "expected {label} to be retryable");
    }
    for (label, err) in is_retryable_false_cases() {
        assert!(!err.is_retryable(), "expected {label} to be non-retryable");
    }
}

// ─── retry_after() ─────────────────────────────────────────────────────

#[test]
fn retry_after_returns_duration_when_rate_limit() {
    let err = rate_limit_with_retry_after();
    assert_eq!(err.retry_after(), Some(Duration::from_secs(30)));
}

#[test]
fn retry_after_returns_none_when_no_header() {
    let err = rate_limit_without_retry_after();
    assert_eq!(err.retry_after(), None);
}

#[test]
fn retry_after_returns_none_for_non_rate_limit() {
    assert_eq!(bad_request_error().retry_after(), None);
    assert_eq!(io_error().retry_after(), None);
    assert_eq!(transport_error().retry_after(), None);
    assert_eq!(decode_error().retry_after(), None);
    assert_eq!(
        HonchoError::Timeout {
            message: "t".into()
        }
        .retry_after(),
        None
    );
}

/// Acceptance #2: `PartialFailure` delegates `retry_after()` to its inner
/// error. A `PartialFailure(RateLimit{30s})` must surface the 30s back through
/// the delegation chain.
#[test]
fn retry_after_delegates_through_partial_failure() {
    let err = partial_failure_rate_limit_error();
    assert_eq!(err.retry_after(), Some(Duration::from_secs(30)));
}

#[test]
fn retry_after_none_when_inner_has_none() {
    let err = partial_failure_server_error();
    assert_eq!(err.retry_after(), None);
}

// ─── is_partial_failure() ──────────────────────────────────────────────

#[test]
fn is_partial_failure_true_for_partial_failure() {
    assert!(partial_failure_server_error().is_partial_failure());
    assert!(partial_failure_timeout_error().is_partial_failure());
}

#[test]
fn is_partial_failure_false_for_other_variants() {
    assert!(!bad_request_error().is_partial_failure());
    assert!(!io_error().is_partial_failure());
    assert!(!rate_limit_without_retry_after().is_partial_failure());
    assert!(!transport_error().is_partial_failure());
}

// ─── into_partial_failure() ────────────────────────────────────────────

/// Extracts the inner error and verifies its properties survive the
/// round-trip. The `messages` Vec cannot be populated with real `Message`
/// values from this external test (`Message::from_raw` is `pub(crate)`), so
/// we assert the structural contract: the tuple is `Some`, the inner `code`
/// and `message` are preserved, and the returned Vec length is consistent
/// with the fixture's `sent` field.
#[test]
fn into_partial_failure_extracts() {
    let err = partial_failure_server_error();
    let (messages, inner) = err
        .into_partial_failure()
        .expect("PartialFailure must extract");
    // Structural consistency: empty messages => sent == 0 (item 11).
    assert!(messages.is_empty());
    // Inner error survives the round-trip with full fidelity.
    assert_eq!(inner.code(), "server_error");
    assert_eq!(inner.message(), "server boom");
    assert_eq!(inner.status_code(), Some(500));
}

#[test]
fn into_partial_failure_none_for_other_variants() {
    assert!(bad_request_error().into_partial_failure().is_none());
    assert!(io_error().into_partial_failure().is_none());
    assert!(transport_error().into_partial_failure().is_none());
}

// ─── Io variant #[from] conversion ─────────────────────────────────────

#[test]
fn io_from_std_io_error() {
    let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
    let honcho_err: HonchoError = io_err.into();
    assert_eq!(honcho_err.code(), "io_error");
    assert_eq!(honcho_err.message(), "I/O error");
    assert!(honcho_err.status_code().is_none());
    assert!(!honcho_err.is_retryable());
}

// ─── from_response mapping ─────────────────────────────────────────────
//
// Covers every `from_response` arm: mapped 4xx, 429+Retry-After, 5xx→Server,
// unmapped-4xx→Client, 3xx→Client (exact message), and the `_` catch-all.

#[test]
fn from_response_3xx_maps_to_client_error() {
    // Item 1: exact-equality assert (not weak `contains`) — guards against
    // wrong/duplicated status codes slipping through.
    let err = from_response(
        reqwest::StatusCode::from_u16(301).unwrap(),
        &reqwest::header::HeaderMap::new(),
        &bytes::Bytes::from("moved"),
        chrono::Utc::now(),
    );
    assert_eq!(err.code(), "client_error");
    assert_eq!(err.status_code(), Some(301));
    assert_eq!(err.message(), "unexpected redirect status 301");
}

#[test]
fn from_response_429_maps_to_rate_limit_with_retry_after() {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::RETRY_AFTER,
        reqwest::header::HeaderValue::from_static("30"),
    );
    let err = from_response(
        reqwest::StatusCode::from_u16(429).unwrap(),
        &headers,
        &bytes::Bytes::from("slow down"),
        chrono::Utc::now(),
    );
    assert_eq!(err.code(), "rate_limit_exceeded");
    assert_eq!(err.status_code(), Some(429));
    assert_eq!(err.retry_after(), Some(Duration::from_secs(30)));
}

#[test]
fn from_response_5xx_maps_to_server() {
    let err = from_response(
        reqwest::StatusCode::from_u16(503).unwrap(),
        &reqwest::header::HeaderMap::new(),
        &bytes::Bytes::from("unavailable"),
        chrono::Utc::now(),
    );
    assert_eq!(err.code(), "server_error");
    assert_eq!(err.status_code(), Some(503));
    assert!(err.is_retryable());
}

#[test]
fn from_response_unmapped_4xx_maps_to_client() {
    // 405 is not in the explicit switch — falls through to the
    // `(400..500).contains(&s)` arm.
    let err = from_response(
        reqwest::StatusCode::from_u16(405).unwrap(),
        &reqwest::header::HeaderMap::new(),
        &bytes::Bytes::from("method not allowed"),
        chrono::Utc::now(),
    );
    assert_eq!(err.code(), "client_error");
    assert_eq!(err.status_code(), Some(405));
    assert!(!err.is_retryable());
}

#[test]
fn from_response_catch_all_maps_to_client() {
    // Status 100 (Continue) is not 3xx/4xx/5xx — hits the `_` arm.
    let err = from_response(
        reqwest::StatusCode::from_u16(100).unwrap(),
        &reqwest::header::HeaderMap::new(),
        &bytes::Bytes::from("info"),
        chrono::Utc::now(),
    );
    assert_eq!(err.code(), "client_error");
    assert_eq!(err.status_code(), Some(100));
    // The `_` arm formats the raw status number, consistent with the 3xx arm
    // (no canonical reason phrase).
    assert_eq!(err.message(), "unexpected response status 100");
}
