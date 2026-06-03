#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

use std::time::Duration;

use honcho_ai::error::HonchoError;

fn msg_error() -> HonchoError {
    HonchoError::BadRequest {
        message: "bad input".into(),
        body: None,
    }
}

fn msg_error_with_status() -> HonchoError {
    HonchoError::NotFound {
        message: "not found".into(),
    }
}

fn partial_failure_error() -> HonchoError {
    HonchoError::PartialFailure {
        messages: vec![],
        sent: 5,
        error: Box::new(HonchoError::Server {
            status: 500,
            message: "server boom".into(),
        }),
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

// --- message() tests ---

#[test]
fn message_bad_request() {
    let err = msg_error();
    assert_eq!(err.message(), "bad input");
}

#[test]
fn message_not_found() {
    let err = msg_error_with_status();
    assert_eq!(err.message(), "not found");
}

#[test]
fn message_authentication() {
    let err = HonchoError::Authentication {
        message: "no key".into(),
    };
    assert_eq!(err.message(), "no key");
}

#[test]
fn message_permission_denied() {
    let err = HonchoError::PermissionDenied {
        message: "forbidden".into(),
    };
    assert_eq!(err.message(), "forbidden");
}

#[test]
fn message_conflict() {
    let err = HonchoError::Conflict {
        message: "dup".into(),
        body: None,
    };
    assert_eq!(err.message(), "dup");
}

#[test]
fn message_rate_limit() {
    let err = rate_limit_without_retry_after();
    assert_eq!(err.message(), "slow down");
}

#[test]
fn message_timeout() {
    let err = HonchoError::Timeout {
        message: "timed out".into(),
    };
    assert_eq!(err.message(), "timed out");
}

#[test]
fn message_connection() {
    let err = HonchoError::Connection {
        message: "refused".into(),
    };
    assert_eq!(err.message(), "refused");
}

#[test]
fn message_configuration() {
    let err = HonchoError::Configuration("bad config".into());
    assert_eq!(err.message(), "bad config");
}

#[test]
fn message_validation() {
    let err = HonchoError::Validation("bad val".into());
    assert_eq!(err.message(), "bad val");
}

#[test]
fn message_io() {
    let err = io_error();
    assert_eq!(err.message(), "I/O error");
}

#[test]
fn message_partial_failure_delegates() {
    let err = partial_failure_error();
    assert_eq!(err.message(), "server boom");
}

// --- status_code() tests ---

#[test]
fn status_code_bad_request() {
    assert_eq!(msg_error().status_code(), Some(400));
}

#[test]
fn status_code_not_found() {
    assert_eq!(msg_error_with_status().status_code(), Some(404));
}

#[test]
fn status_code_rate_limit() {
    assert_eq!(rate_limit_without_retry_after().status_code(), Some(429));
}

#[test]
fn status_code_server() {
    let err = HonchoError::Server {
        status: 503,
        message: "down".into(),
    };
    assert_eq!(err.status_code(), Some(503));
}

#[test]
fn status_code_client_unmapped() {
    let err = HonchoError::Client {
        status: 418,
        message: "teapot".into(),
    };
    assert_eq!(err.status_code(), Some(418));
}

#[test]
fn status_code_timeout_none() {
    let err = HonchoError::Timeout {
        message: "t".into(),
    };
    assert_eq!(err.status_code(), None);
}

#[test]
fn status_code_io_none() {
    assert_eq!(io_error().status_code(), None);
}

#[test]
fn status_code_validation_none() {
    let err = HonchoError::Validation("v".into());
    assert_eq!(err.status_code(), None);
}

#[test]
fn status_code_configuration_none() {
    let err = HonchoError::Configuration("c".into());
    assert_eq!(err.status_code(), None);
}

#[test]
fn status_code_partial_failure_delegates() {
    let err = partial_failure_error();
    assert_eq!(err.status_code(), Some(500));
}

// --- is_partial_failure() tests ---

#[test]
fn is_partial_failure_true() {
    assert!(partial_failure_error().is_partial_failure());
}

#[test]
fn is_partial_failure_false_for_other() {
    assert!(!msg_error().is_partial_failure());
    assert!(!io_error().is_partial_failure());
    assert!(!rate_limit_without_retry_after().is_partial_failure());
}

// --- into_partial_failure() tests ---

#[test]
fn into_partial_failure_extracts() {
    let err = partial_failure_error();
    let (messages, inner) = err.into_partial_failure().unwrap();
    assert!(messages.is_empty());
    assert_eq!(inner.code(), "server_error");
    assert_eq!(inner.message(), "server boom");
}

#[test]
fn into_partial_failure_none_for_other() {
    let err = msg_error();
    assert!(err.into_partial_failure().is_none());
}

#[test]
fn into_partial_failure_none_for_io() {
    let err = io_error();
    assert!(err.into_partial_failure().is_none());
}

// --- code() tests ---

#[test]
fn code_all_variants() {
    assert_eq!(msg_error().code(), "bad_request");
    assert_eq!(msg_error_with_status().code(), "not_found");
    assert_eq!(
        HonchoError::Authentication {
            message: "a".into()
        }
        .code(),
        "authentication_error"
    );
    assert_eq!(
        HonchoError::PermissionDenied {
            message: "p".into()
        }
        .code(),
        "permission_denied"
    );
    assert_eq!(
        HonchoError::Conflict {
            message: "c".into(),
            body: None
        }
        .code(),
        "conflict"
    );
    assert_eq!(
        HonchoError::UnprocessableEntity {
            message: "u".into(),
            body: None
        }
        .code(),
        "unprocessable_entity"
    );
    assert_eq!(
        rate_limit_without_retry_after().code(),
        "rate_limit_exceeded"
    );
    assert_eq!(
        HonchoError::Client {
            status: 418,
            message: "t".into()
        }
        .code(),
        "client_error"
    );
    assert_eq!(
        HonchoError::Server {
            status: 500,
            message: "s".into()
        }
        .code(),
        "server_error"
    );
    assert_eq!(
        HonchoError::Timeout {
            message: "t".into()
        }
        .code(),
        "timeout"
    );
    assert_eq!(
        HonchoError::Connection {
            message: "c".into()
        }
        .code(),
        "connection_error"
    );
    assert_eq!(
        HonchoError::Configuration("c".into()).code(),
        "configuration_error"
    );
    assert_eq!(
        HonchoError::Validation("v".into()).code(),
        "validation_error"
    );
    assert_eq!(io_error().code(), "io_error");
    assert_eq!(partial_failure_error().code(), "partial_failure");
}

// --- retry_after() tests ---

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
    assert_eq!(msg_error().retry_after(), None);
    assert_eq!(io_error().retry_after(), None);
}

// --- Io variant #[from] conversion ---

#[test]
fn io_from_std_io_error() {
    let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
    let honcho_err: HonchoError = io_err.into();
    assert_eq!(honcho_err.code(), "io_error");
    assert_eq!(honcho_err.message(), "I/O error");
    assert!(honcho_err.status_code().is_none());
}

// --- 3xx redirect mapping in from_response ---

#[test]
fn from_response_3xx_maps_to_client_error() {
    let err = honcho_ai::error::from_response(
        reqwest::StatusCode::from_u16(301).unwrap(),
        &reqwest::header::HeaderMap::new(),
        &bytes::Bytes::from("moved"),
        chrono::Utc::now(),
    );
    assert_eq!(err.code(), "client_error");
    assert_eq!(err.status_code(), Some(301));
    assert!(err.message().contains("unexpected redirect"));
}
