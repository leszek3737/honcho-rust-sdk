#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

use honcho_ai::HonchoError;
use honcho_ai::types::session::{SessionContextOptions, SessionCreate};

/// Assert a `validate()` result is the typed `Validation` error variant whose
/// message mentions `needle` — pinning the *cause*, not just "some error" the
/// way a bare `assert!(is_err())` would (which passes on the wrong rejection).
fn expect_validation(result: Result<(), HonchoError>, needle: &str) {
    let err = result.unwrap_err();
    assert!(
        matches!(err, HonchoError::Validation(_)),
        "expected HonchoError::Validation, got {err:?}"
    );
    assert_eq!(err.code(), "validation_error");
    assert!(
        err.message().contains(needle),
        "validation message {:?} does not contain {:?}",
        err.message(),
        needle
    );
}

/// Assert a `validate()` result is `Ok`.
fn expect_ok(result: Result<(), HonchoError>) {
    assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
}

// --- SessionCreate::validate() ---

#[test]
fn session_create_validate_ok() {
    expect_ok(
        SessionCreate::builder()
            .id("valid-session_1")
            .build()
            .validate(),
    );
}

#[test]
fn session_create_validate_hyphen_ok() {
    expect_ok(SessionCreate::builder().id("my-session").build().validate());
}

#[test]
fn session_create_validate_underscore_ok() {
    expect_ok(SessionCreate::builder().id("my_session").build().validate());
}

#[test]
fn session_create_validate_empty_id() {
    let sc = SessionCreate::builder().id("").build();
    expect_validation(sc.validate(), "must not be empty");
}

#[test]
fn session_create_validate_invalid_chars() {
    let sc = SessionCreate::builder().id("has space").build();
    expect_validation(sc.validate(), "[a-zA-Z0-9_-]");
}

#[test]
fn session_create_validate_special_chars() {
    let sc = SessionCreate::builder().id("has@special!").build();
    expect_validation(sc.validate(), "[a-zA-Z0-9_-]");
}

#[test]
fn session_create_validate_non_ascii_id() {
    // Non-ASCII letters fall outside [a-zA-Z0-9_-]; the ASCII-only contract must
    // reject them (locks behaviour against an accidental `is_alphanumeric` swap).
    for id in ["café", "日本"] {
        let sc = SessionCreate::builder().id(id).build();
        expect_validation(sc.validate(), "[a-zA-Z0-9_-]");
    }
}

// --- SessionContextOptions::validate() ---

#[test]
fn ctx_validate_ok_empty() {
    expect_ok(SessionContextOptions::builder().build().validate());
}

#[test]
fn ctx_validate_perspective_requires_target() {
    let opts = SessionContextOptions::builder()
        .peer_perspective("alice")
        .build();
    expect_validation(opts.validate(), "peer_perspective requires peer_target");
}

#[test]
fn ctx_validate_search_query_requires_target() {
    let opts = SessionContextOptions::builder()
        .search_query("preferences")
        .build();
    expect_validation(opts.validate(), "search_query requires peer_target");
}

#[test]
fn ctx_validate_search_query_with_target_ok() {
    // Companion field present → the cross-field rule is satisfied.
    let opts = SessionContextOptions::builder()
        .peer_target("bob")
        .search_query("preferences")
        .build();
    expect_ok(opts.validate());
}

#[test]
fn ctx_validate_both_target_and_perspective_ok() {
    let opts = SessionContextOptions::builder()
        .peer_target("bob")
        .peer_perspective("alice")
        .build();
    expect_ok(opts.validate());
}

#[test]
fn ctx_validate_search_top_k_too_high() {
    let opts = SessionContextOptions::builder()
        .peer_target("bob")
        .search_top_k(101)
        .build();
    expect_validation(opts.validate(), "search_top_k");
}

#[test]
fn ctx_validate_search_top_k_zero() {
    let opts = SessionContextOptions::builder()
        .peer_target("bob")
        .search_top_k(0)
        .build();
    expect_validation(opts.validate(), "search_top_k");
}

#[test]
fn ctx_validate_search_top_k_without_target_still_fires() {
    // Range validation is independent of the peer_target cross-field rule: an
    // out-of-range value is rejected even when peer_target is absent.
    let opts = SessionContextOptions::builder().search_top_k(200).build();
    expect_validation(opts.validate(), "search_top_k");
}

#[test]
fn ctx_validate_search_max_distance_too_high() {
    let opts = SessionContextOptions::builder()
        .peer_target("bob")
        .search_max_distance(1.5)
        .build();
    expect_validation(opts.validate(), "search_max_distance");
}

#[test]
fn ctx_validate_search_max_distance_negative() {
    let opts = SessionContextOptions::builder()
        .peer_target("bob")
        .search_max_distance(-0.1)
        .build();
    expect_validation(opts.validate(), "search_max_distance");
}

#[test]
fn ctx_validate_search_max_distance_nan() {
    // NaN is outside [0.0, 1.0] (every NaN comparison is false) → rejected.
    let opts = SessionContextOptions::builder()
        .peer_target("bob")
        .search_max_distance(f64::NAN)
        .build();
    expect_validation(opts.validate(), "search_max_distance");
}

#[test]
fn ctx_validate_max_conclusions_too_high() {
    let opts = SessionContextOptions::builder()
        .peer_target("bob")
        .max_conclusions(101)
        .build();
    expect_validation(opts.validate(), "max_conclusions");
}

#[test]
fn ctx_validate_max_conclusions_zero() {
    // MIN = 1, so 0 is out of range — symmetric with search_top_k(0).
    let opts = SessionContextOptions::builder()
        .peer_target("bob")
        .max_conclusions(0)
        .build();
    expect_validation(opts.validate(), "max_conclusions");
}

#[test]
fn ctx_validate_tokens_zero() {
    let opts = SessionContextOptions::builder().tokens(0).build();
    expect_validation(opts.validate(), "tokens must be greater than 0");
}

#[test]
fn ctx_validate_tokens_nonzero_ok() {
    expect_ok(
        SessionContextOptions::builder()
            .tokens(4096)
            .build()
            .validate(),
    );
}

#[test]
fn ctx_validate_boundary_values_min() {
    let opts = SessionContextOptions::builder()
        .peer_target("bob")
        .search_top_k(1)
        .search_max_distance(0.0)
        .max_conclusions(1)
        .build();
    expect_ok(opts.validate());
}

#[test]
fn ctx_validate_boundary_values_max() {
    let opts = SessionContextOptions::builder()
        .peer_target("bob")
        .search_top_k(100)
        .search_max_distance(1.0)
        .max_conclusions(100)
        .build();
    expect_ok(opts.validate());
}
