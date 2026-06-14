#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::manual_range_contains,
    missing_docs
)]

//! Tests for `from_response` status→variant mapping, `parse_error_body`,
//! `parse_retry_after`, and the `is_retryable()` policy.
//!
//! `code()` coverage is split with `tests/error_methods.rs`:
//! this file owns the `from_response`-reachable (HTTP-status-derived) variants;
//! `error_methods.rs` owns the constructed-directly variants
//! (`Timeout`, `Connection`, `Transport`, `Decode`, `Io`, `Configuration`,
//! `Validation`, `PartialFailure`).

use std::error::Error;
use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use honcho_ai::error::{HonchoError, from_response, parse_error_body, parse_retry_after};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use reqwest::header::{HeaderMap, HeaderValue};
use rstest::rstest;
use static_assertions::assert_impl_all;

// Deterministic timestamp for tests where the instant is irrelevant.
fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap()
}

fn status(code: u16) -> StatusCode {
    StatusCode::from_u16(code).unwrap()
}

fn body(s: &str) -> bytes::Bytes {
    bytes::Bytes::copy_from_slice(s.as_bytes())
}

fn from_resp(code: u16, body_str: &str) -> HonchoError {
    from_response(status(code), &HeaderMap::new(), &body(body_str), now())
}

// === status → variant mapping ===

#[rstest]
#[case(400, "bad_request")]
#[case(401, "authentication_error")]
#[case(403, "permission_denied")]
#[case(404, "not_found")]
#[case(409, "conflict")]
#[case(422, "unprocessable_entity")]
fn status_maps_to_variant(#[case] status: u16, #[case] expected_code: &str) {
    let err = from_resp(status, r#"{"message":"test error"}"#);

    assert_eq!(err.code(), expected_code);
    match status {
        400 => assert!(matches!(err, HonchoError::BadRequest { .. })),
        401 => assert!(matches!(err, HonchoError::Authentication { .. })),
        403 => assert!(matches!(err, HonchoError::PermissionDenied { .. })),
        404 => assert!(matches!(err, HonchoError::NotFound { .. })),
        409 => assert!(matches!(err, HonchoError::Conflict { .. })),
        422 => assert!(matches!(err, HonchoError::UnprocessableEntity { .. })),
        _ => panic!("unexpected status"),
    }
}

#[rstest]
#[case(500)]
#[case(502)]
#[case(503)]
#[case(504)]
fn server_5xx_maps_to_server_with_status(#[case] status: u16) {
    let err = from_resp(status, "internal server error");

    assert!(matches!(
        err,
        HonchoError::Server {
            status: s,
            ..
        } if s == status
    ));
    assert_eq!(err.code(), "server_error");
}

#[rstest]
#[case(405)]
#[case(408)]
#[case(413)]
#[case(418)]
fn unmapped_4xx_maps_to_client_with_status(#[case] status: u16) {
    let err = from_resp(status, "client error");

    assert!(matches!(
        err,
        HonchoError::Client {
            status: s,
            ..
        } if s == status
    ));
    assert_eq!(err.code(), "client_error");
}

// === rate limit / Retry-After header parsing via from_response ===

#[test]
fn rate_limit_429_parses_retry_after_seconds() {
    let mut headers = HeaderMap::new();
    headers.insert("retry-after", HeaderValue::from_static("7"));
    let err = from_response(
        StatusCode::TOO_MANY_REQUESTS,
        &headers,
        &body(r#"{"message":"rate limited"}"#),
        now(),
    );

    match err {
        HonchoError::RateLimit { retry_after, .. } => {
            assert_eq!(retry_after, Some(Duration::from_secs(7)));
        }
        _ => panic!("expected RateLimit, got {err:?}"),
    }
}

#[test]
fn rate_limit_429_parses_retry_after_http_date() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "retry-after",
        HeaderValue::from_static("Thu, 01 Jan 2026 00:00:05 GMT"),
    );
    let err = from_response(
        StatusCode::TOO_MANY_REQUESTS,
        &headers,
        &body(r#"{"message":"rate limited"}"#),
        now(),
    );

    match err {
        HonchoError::RateLimit {
            retry_after: Some(dur),
            ..
        } => {
            let secs = dur.as_secs_f64();
            assert!((4.9..=5.1).contains(&secs), "expected ~5s, got {secs}s");
        }
        _ => panic!("expected RateLimit with retry_after, got {err:?}"),
    }
}

#[test]
fn rate_limit_429_without_retry_after_is_none() {
    let err = from_resp(429, r#"{"message":"rate limited"}"#);

    match err {
        HonchoError::RateLimit {
            retry_after: None, ..
        } => {}
        _ => panic!("expected RateLimit with None retry_after, got {err:?}"),
    }
}

#[test]
fn retry_after_with_garbage_returns_none() {
    let mut headers = HeaderMap::new();
    headers.insert("retry-after", HeaderValue::from_static("not-a-valid-value"));
    let result = parse_retry_after(headers.get("retry-after").unwrap(), now());
    assert!(result.is_none());
}

// === parse_retry_after direct cases (panic regression + clamp + dates) ===

#[rstest]
#[case("7", Some(Duration::from_secs(7)))]
#[case("0", Some(Duration::ZERO))]
#[case("3.5", Some(Duration::from_millis(3500)))]
fn parse_retry_after_seconds_valid(#[case] raw: &str, #[case] expected: Option<Duration>) {
    let mut headers = HeaderMap::new();
    headers.insert("retry-after", HeaderValue::from_str(raw).unwrap());
    let result = parse_retry_after(headers.get("retry-after").unwrap(), now());
    assert_eq!(result, expected);
}

#[rstest]
#[case("inf")]
#[case("infinity")]
#[case("1e300")]
// Non-finite values that `f64::max(0.0)` would silently coerce to 0.0 (it
// ignores NaN and returns the other operand) — must be rejected as None, not
// turned into "retry immediately".
#[case("nan")]
#[case("NaN")]
#[case("-inf")]
#[case("-infinity")]
fn parse_retry_after_overflow_returns_none_not_panic(#[case] raw: &str) {
    // Regression: previously `Duration::from_secs_f64` panicked on inf/overflow.
    // Agreed contract: non-finite/overflow → None. MUST NOT panic.
    let mut headers = HeaderMap::new();
    headers.insert("retry-after", HeaderValue::from_str(raw).unwrap());
    let result = parse_retry_after(headers.get("retry-after").unwrap(), now());
    assert_eq!(
        result, None,
        "non-finite/overflow should be None, not panic"
    );
}

#[test]
fn parse_retry_after_negative_clamps_to_zero() {
    let mut headers = HeaderMap::new();
    headers.insert("retry-after", HeaderValue::from_static("-5"));
    let result = parse_retry_after(headers.get("retry-after").unwrap(), now());
    assert_eq!(result, Some(Duration::ZERO));
}

#[test]
fn parse_retry_after_past_http_date_is_zero() {
    // 01 Jan 2025 is a Wednesday; `now()` is 01 Jan 2026, so this is in the past.
    let mut headers = HeaderMap::new();
    headers.insert(
        "retry-after",
        HeaderValue::from_static("Wed, 01 Jan 2025 00:00:00 GMT"),
    );
    let result = parse_retry_after(headers.get("retry-after").unwrap(), now());
    assert_eq!(result, Some(Duration::ZERO));
}

#[test]
fn parse_retry_after_future_http_date_returns_positive_diff() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "retry-after",
        HeaderValue::from_static("Thu, 01 Jan 2026 00:00:05 GMT"),
    );
    let result = parse_retry_after(headers.get("retry-after").unwrap(), now());
    assert_eq!(result, Some(Duration::from_secs(5)));
}

// === parse_error_body ===

#[test]
fn error_body_extracts_message_field_priority() {
    // detail > message > error
    let (msg, body_value) =
        parse_error_body(r#"{"detail":"d","message":"m","error":"e"}"#.as_bytes());
    assert_eq!(msg, "d");
    assert!(body_value.is_some());

    let (msg, body_value) = parse_error_body(r#"{"message":"m","error":"e"}"#.as_bytes());
    assert_eq!(msg, "m");
    assert!(body_value.is_some());

    let (msg, body_value) = parse_error_body(r#"{"error":"e"}"#.as_bytes());
    assert_eq!(msg, "e");
    assert!(body_value.is_some());

    let (msg, body_value) = parse_error_body(r#""plain string""#.as_bytes());
    assert_eq!(msg, "plain string");
    assert!(body_value.is_some());
}

#[test]
fn error_body_fastapi_422_array_detail_yields_readable_message() {
    // FastAPI 422 returns detail as an array of objects. A1's `detail_message`
    // helper joins the `msg` fields with `"; "`, producing a human-readable
    // message instead of dumping raw JSON.
    let raw = r#"{"detail":[{"loc":["body","x"],"msg":"field required"}]}"#;
    let (msg, body_value) = parse_error_body(raw.as_bytes());
    assert_eq!(
        msg, "field required",
        "readable form (detail_message helper)"
    );
    assert!(
        body_value.is_some(),
        "valid JSON must round-trip body_value"
    );
    let value = body_value.unwrap();
    assert!(
        value.get("detail").and_then(|v| v.as_array()).is_some(),
        "body should preserve the detail array"
    );
}

#[test]
fn error_body_invalid_json_returns_lossy_message_and_no_body() {
    let raw = "not valid json {";
    let (msg, body_value) = parse_error_body(raw.as_bytes());
    assert_eq!(msg, raw);
    assert!(body_value.is_none(), "invalid JSON must yield None body");
}

#[test]
fn error_body_empty_object_returns_json_string_and_body() {
    let raw = "{}";
    let (msg, body_value) = parse_error_body(raw.as_bytes());
    assert_eq!(msg, raw);
    assert!(body_value.is_some());
}

// === display: strict "HTTP {status}" prefix ===

#[rstest]
#[case(400, "Honcho API error: HTTP 400 something went wrong")]
#[case(401, "Honcho API error: HTTP 401 something went wrong")]
#[case(404, "Honcho API error: HTTP 404 something went wrong")]
#[case(500, "Honcho API error: HTTP 500 something went wrong")]
fn display_includes_status_and_message_strict_prefix(#[case] code: u16, #[case] expected: &str) {
    let err = from_resp(code, r#"{"message":"something went wrong"}"#);
    let display = format!("{err}");
    assert_eq!(display, expected);
}

// === non-JSON body path: pin lossy-fallback text ===

#[test]
fn non_json_body_uses_lossy_fallback_text() {
    let err = from_resp(500, "internal server error");
    match err {
        HonchoError::Server { message, .. } => {
            assert_eq!(message, "internal server error");
        }
        _ => panic!("expected Server, got {err:?}"),
    }
}

// === code() — from_response-reachable variants (A2 split) ===
//
// Constructed-directly variants (Timeout, Connection, Transport, Decode, Io,
// Configuration, Validation, PartialFailure) are covered in tests/error_methods.rs.

#[test]
fn code_from_response_reachable_variants() {
    let cases: [(&HonchoError, &str); 9] = [
        (
            &HonchoError::BadRequest {
                message: String::new(),
                body: None,
            },
            "bad_request",
        ),
        (
            &HonchoError::Authentication {
                message: String::new(),
            },
            "authentication_error",
        ),
        (
            &HonchoError::PermissionDenied {
                message: String::new(),
            },
            "permission_denied",
        ),
        (
            &HonchoError::NotFound {
                message: String::new(),
            },
            "not_found",
        ),
        (
            &HonchoError::Conflict {
                message: String::new(),
                body: None,
            },
            "conflict",
        ),
        (
            &HonchoError::UnprocessableEntity {
                message: String::new(),
                body: None,
            },
            "unprocessable_entity",
        ),
        (
            &HonchoError::RateLimit {
                message: String::new(),
                retry_after: None,
            },
            "rate_limit_exceeded",
        ),
        (
            &HonchoError::Server {
                status: 500,
                message: String::new(),
            },
            "server_error",
        ),
        (
            &HonchoError::Client {
                status: 405,
                message: String::new(),
            },
            "client_error",
        ),
    ];

    for (err, expected) in cases {
        assert_eq!(err.code(), expected, "mismatch for {expected}");
    }
}

// === is_retryable policy matrix ===

#[rstest]
#[case(HonchoError::RateLimit { message: String::new(), retry_after: None }, true)]
#[case(HonchoError::Server { status: 500, message: String::new() }, true)]
#[case(HonchoError::Server { status: 502, message: String::new() }, true)]
#[case(HonchoError::Server { status: 503, message: String::new() }, true)]
#[case(HonchoError::Server { status: 504, message: String::new() }, true)]
#[case(HonchoError::Server { status: 501, message: String::new() }, false)]
#[case(HonchoError::BadRequest { message: String::new(), body: None }, false)]
#[case(HonchoError::Authentication { message: String::new() }, false)]
#[case(HonchoError::PermissionDenied { message: String::new() }, false)]
#[case(HonchoError::NotFound { message: String::new() }, false)]
#[case(HonchoError::Conflict { message: String::new(), body: None }, false)]
#[case(HonchoError::UnprocessableEntity { message: String::new(), body: None }, false)]
#[case(HonchoError::Client { status: 405, message: String::new() }, false)]
#[case(HonchoError::Timeout { message: String::new() }, true)]
#[case(HonchoError::Connection { message: String::new() }, true)]
// Constructed-directly non-status variants:
#[case(HonchoError::Configuration("bad".into()), false)]
#[case(HonchoError::Validation("bad".into()), false)]
fn retryable_policy_matches_http_client(#[case] err: HonchoError, #[case] expected: bool) {
    assert_eq!(err.is_retryable(), expected);
}

#[test]
fn retryable_decode_and_io_variants_are_false() {
    let json_err = serde_json::from_str::<Vec<i32>>("{}").unwrap_err();
    let decode = HonchoError::Decode {
        path: "root".into(),
        source: json_err,
    };
    assert!(!decode.is_retryable());

    let io = std::io::Error::other("boom");
    let io_err = HonchoError::Io(io);
    assert!(!io_err.is_retryable());
}

#[test]
fn partial_failure_is_never_retryable_even_if_inner_is() {
    // Agreed contract: PartialFailure → false regardless of inner error kind.
    // Proves consistency vs the old accidental mix where {Server 503} was
    // retryable via status_code() delegation but {Timeout} was not.
    let partial_server_503 = HonchoError::PartialFailure {
        messages: vec![],
        sent: 0,
        error: Box::new(HonchoError::Server {
            status: 503,
            message: "boom".into(),
        }),
    };
    assert!(
        !partial_server_503.is_retryable(),
        "PartialFailure wrapping Server 503 must not be retryable"
    );

    let partial_timeout = HonchoError::PartialFailure {
        messages: vec![],
        sent: 0,
        error: Box::new(HonchoError::Timeout {
            message: "slow".into(),
        }),
    };
    assert!(
        !partial_timeout.is_retryable(),
        "PartialFailure wrapping Timeout must not be retryable"
    );
}

// === Transport variant: non-retryable + source chain ===
//
// Constructed via URL parse failure — no real socket is opened; the request
// builder rejects the malformed URL before any I/O.

#[tokio::test]
async fn transport_error_from_url_parse_is_not_retryable_with_source() {
    let transport_err: HonchoError = reqwest::Client::new()
        .get("ht!tp://[invalid")
        .send()
        .await
        .unwrap_err()
        .into();
    assert!(!transport_err.is_retryable(), "Transport is non-retryable");
    assert!(
        transport_err.source().is_some(),
        "Transport wraps reqwest::Error (source chain)"
    );
}

// === Serialization variant: code / status_code / message ===

#[test]
fn serialization_variant_code_status_and_message() {
    // A trivial serde failure to populate the `source` field.
    let source = serde_json::from_str::<i32>("x").unwrap_err();
    let err = HonchoError::Serialization {
        path: "X".into(),
        source,
    };

    assert_eq!(err.code(), "serialization_error");
    assert_eq!(err.status_code(), None, "Serialization has no HTTP status");
    assert_eq!(err.message(), "failed to serialize request");
}

// === §169: a genuine serialize failure maps to `Serialization` (not Decode) ===
//
// The variant above is fed a *parse* error for brevity; this test pins the
// behavioural intent of the serialize-error recategorization: a real `serde_json` *serialize*
// failure (a map whose keys do not serialize to strings) must land in
// `Serialization`, never `Decode`/`Configuration`. End-to-end through the public
// API this path cannot be reached (metadata/config are `HashMap<String, Value>`,
// which always serialize), so the failure is constructed at the serde boundary
// and the static recategorization is verified at the call sites.
//
// `serde_json` classifies a "key must be a string" error as `Category::Syntax`
// (same bucket as a parse error), so `classify()` alone cannot tell serialize
// from parse — the distinguishing, serializer-only signal is the Display text,
// which for a serializer error carries no `line/column` suffix.
#[test]
fn genuine_serialize_failure_maps_to_serialization_variant() {
    use std::collections::BTreeMap;

    // A non-string map key forces a real serialize failure out of `serde_json`.
    let mut bad: BTreeMap<(i32, i32), i32> = BTreeMap::new();
    bad.insert((1, 2), 3);
    let source = serde_json::to_string(&bad)
        .expect_err("tuple-keyed map must fail to serialize as a JSON object");

    // Proves the source is a *serialize* error, not a parse error: this exact
    // message is produced only by the serializer's map-key path.
    assert_eq!(
        source.to_string(),
        "key must be a string",
        "expected the serializer's key error, not a parse/syntax error"
    );

    let err = HonchoError::Serialization {
        path: "metadata".into(),
        source,
    };

    assert!(matches!(err, HonchoError::Serialization { .. }));
    assert_eq!(err.code(), "serialization_error");
    assert_eq!(err.status_code(), None);
    assert_eq!(err.message(), "failed to serialize request");

    // The wrapped source survives in the chain and is still the serialize error.
    let chained = err.source().expect("Serialization wraps a source");
    assert_eq!(
        chained.to_string(),
        "key must be a string",
        "the serialize error must be preserved as the variant's source"
    );
}

// === parity: every HonchoError variant exercises code/status_code/message ===
//
// Single source of truth for the full variant matrix: asserts `code()` and
// `status_code()` for all 18 variants (including the new `Serialization`) and
// exercises `message()` on each. Complements the from_response-reachable
// `code()` check above and the constructed-directly coverage in
// tests/error_methods.rs.
//
// Exhaustive table-style parity test: the length is the inline `cases` vec with
// one entry per `HonchoError` variant. Splitting it would fragment the single
// source of truth, so the line cap is waived deliberately.
#[allow(clippy::too_many_lines)]
#[test]
fn every_variant_code_status_message_parity() {
    fn serde_err() -> serde_json::Error {
        serde_json::from_str::<i32>("x").unwrap_err()
    }
    // A `reqwest::Error` without touching the network: the malformed URL is
    // rejected when the request is built.
    fn reqwest_err() -> reqwest::Error {
        reqwest::Client::new()
            .get("ht!tp://[invalid")
            .build()
            .unwrap_err()
    }

    let cases: Vec<(HonchoError, &str, Option<u16>)> = vec![
        (
            HonchoError::BadRequest {
                message: "m".into(),
                body: None,
            },
            "bad_request",
            Some(400),
        ),
        (
            HonchoError::Authentication {
                message: "m".into(),
            },
            "authentication_error",
            Some(401),
        ),
        (
            HonchoError::PermissionDenied {
                message: "m".into(),
            },
            "permission_denied",
            Some(403),
        ),
        (
            HonchoError::NotFound {
                message: "m".into(),
            },
            "not_found",
            Some(404),
        ),
        (
            HonchoError::Conflict {
                message: "m".into(),
                body: None,
            },
            "conflict",
            Some(409),
        ),
        (
            HonchoError::UnprocessableEntity {
                message: "m".into(),
                body: None,
            },
            "unprocessable_entity",
            Some(422),
        ),
        (
            HonchoError::RateLimit {
                message: "m".into(),
                retry_after: None,
            },
            "rate_limit_exceeded",
            Some(429),
        ),
        (
            HonchoError::Client {
                status: 405,
                message: "m".into(),
            },
            "client_error",
            Some(405),
        ),
        (
            HonchoError::Server {
                status: 500,
                message: "m".into(),
            },
            "server_error",
            Some(500),
        ),
        (
            HonchoError::Timeout {
                message: "m".into(),
            },
            "timeout",
            None,
        ),
        (
            HonchoError::Connection {
                message: "m".into(),
            },
            "connection_error",
            None,
        ),
        (
            HonchoError::Transport(reqwest_err()),
            "transport_error",
            None,
        ),
        (
            HonchoError::Decode {
                path: "root".into(),
                source: serde_err(),
            },
            "decode_error",
            None,
        ),
        (
            HonchoError::Serialization {
                path: "X".into(),
                source: serde_err(),
            },
            "serialization_error",
            None,
        ),
        (
            HonchoError::Io(std::io::Error::other("boom")),
            "io_error",
            None,
        ),
        (
            HonchoError::Configuration("c".into()),
            "configuration_error",
            None,
        ),
        (
            HonchoError::Validation("v".into()),
            "validation_error",
            None,
        ),
        (
            // status_code() delegates to the inner error (here: None).
            HonchoError::PartialFailure {
                messages: vec![],
                sent: 0,
                error: Box::new(HonchoError::Validation("inner".into())),
            },
            "partial_failure",
            None,
        ),
    ];

    // 18 variants total — guards against silently forgetting one when the enum
    // grows (it is `#[non_exhaustive]`, so this is a manual completeness check).
    assert_eq!(
        cases.len(),
        18,
        "expected every HonchoError variant covered"
    );

    for (err, expected_code, expected_status) in &cases {
        assert_eq!(
            err.code(),
            *expected_code,
            "code() mismatch for {expected_code}"
        );
        assert_eq!(
            err.status_code(),
            *expected_status,
            "status_code() mismatch for {expected_code}"
        );
        // Exercise message(): must be callable and non-panicking for every
        // variant. It must never be empty.
        assert!(
            !err.message().is_empty(),
            "message() empty for {expected_code}"
        );
    }
}

// === bounds ===

#[test]
fn error_bounds() {
    assert_impl_all!(HonchoError: Send, Sync, Error);
}
