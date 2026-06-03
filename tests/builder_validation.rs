#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

use honcho_ai::types::session::{SessionContextOptions, SessionCreate};

// --- SessionCreate::validate() ---

#[test]
fn session_create_validate_ok() {
    let sc = SessionCreate::builder()
        .id("valid-session_1".to_string())
        .build();
    assert!(sc.validate().is_ok());
}

#[test]
fn session_create_validate_empty_id() {
    let sc = SessionCreate::builder().id(String::new()).build();
    let err = sc.validate().unwrap_err();
    assert_eq!(err.code(), "validation_error");
    assert!(err.message().contains("must not be empty"));
}

#[test]
fn session_create_validate_invalid_chars() {
    let sc = SessionCreate::builder().id("has space".to_string()).build();
    let err = sc.validate().unwrap_err();
    assert_eq!(err.code(), "validation_error");
    assert!(err.message().contains("[a-zA-Z0-9_-]"));
}

#[test]
fn session_create_validate_special_chars() {
    let sc = SessionCreate::builder()
        .id("has@special!".to_string())
        .build();
    assert!(sc.validate().is_err());
}

#[test]
fn session_create_validate_hyphen_ok() {
    let sc = SessionCreate::builder()
        .id("my-session".to_string())
        .build();
    assert!(sc.validate().is_ok());
}

#[test]
fn session_create_validate_underscore_ok() {
    let sc = SessionCreate::builder()
        .id("my_session".to_string())
        .build();
    assert!(sc.validate().is_ok());
}

// --- SessionContextOptions::validate() ---

#[test]
fn session_context_options_validate_ok_empty() {
    let opts = SessionContextOptions::builder().build();
    assert!(opts.validate().is_ok());
}

#[test]
fn session_context_options_validate_perspective_requires_target() {
    let opts = SessionContextOptions::builder()
        .peer_perspective("alice".to_string())
        .build();
    let err = opts.validate().unwrap_err();
    assert_eq!(err.code(), "validation_error");
    assert!(
        err.message()
            .contains("peer_perspective requires peer_target")
    );
}

#[test]
fn session_context_options_validate_search_query_requires_target() {
    let opts = SessionContextOptions::builder()
        .search_query("preferences".to_string())
        .build();
    let err = opts.validate().unwrap_err();
    assert_eq!(err.code(), "validation_error");
    assert!(err.message().contains("search_query requires peer_target"));
}

#[test]
fn session_context_options_validate_both_target_and_perspective_ok() {
    let opts = SessionContextOptions::builder()
        .peer_target("bob".to_string())
        .peer_perspective("alice".to_string())
        .build();
    assert!(opts.validate().is_ok());
}

#[test]
fn session_context_options_validate_search_top_k_too_high() {
    let opts = SessionContextOptions::builder()
        .peer_target("bob".to_string())
        .search_top_k(101u32)
        .build();
    let err = opts.validate().unwrap_err();
    assert_eq!(err.code(), "validation_error");
    assert!(err.message().contains("search_top_k"));
}

#[test]
fn session_context_options_validate_search_top_k_zero() {
    let opts = SessionContextOptions::builder()
        .peer_target("bob".to_string())
        .search_top_k(0u32)
        .build();
    assert!(opts.validate().is_err());
}

#[test]
fn session_context_options_validate_search_max_distance_too_high() {
    let opts = SessionContextOptions::builder()
        .peer_target("bob".to_string())
        .search_max_distance(1.5f64)
        .build();
    assert!(opts.validate().is_err());
}

#[test]
fn session_context_options_validate_search_max_distance_negative() {
    let opts = SessionContextOptions::builder()
        .peer_target("bob".to_string())
        .search_max_distance(-0.1f64)
        .build();
    assert!(opts.validate().is_err());
}

#[test]
fn session_context_options_validate_max_conclusions_too_high() {
    let opts = SessionContextOptions::builder()
        .peer_target("bob".to_string())
        .max_conclusions(101u32)
        .build();
    assert!(opts.validate().is_err());
}

#[test]
fn session_context_options_validate_tokens_zero() {
    let opts = SessionContextOptions::builder().tokens(0u32).build();
    let err = opts.validate().unwrap_err();
    assert_eq!(err.code(), "validation_error");
    assert!(err.message().contains("tokens must be greater than 0"));
}

#[test]
fn session_context_options_validate_tokens_nonzero_ok() {
    let opts = SessionContextOptions::builder().tokens(4096u32).build();
    assert!(opts.validate().is_ok());
}

#[test]
fn session_context_options_validate_boundary_values() {
    let opts = SessionContextOptions::builder()
        .peer_target("bob".to_string())
        .search_top_k(1u32)
        .search_max_distance(0.0f64)
        .max_conclusions(1u32)
        .build();
    assert!(opts.validate().is_ok());
}

#[test]
fn session_context_options_validate_boundary_values_max() {
    let opts = SessionContextOptions::builder()
        .peer_target("bob".to_string())
        .search_top_k(100u32)
        .search_max_distance(1.0f64)
        .max_conclusions(100u32)
        .build();
    assert!(opts.validate().is_ok());
}
