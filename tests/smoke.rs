#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

use honcho_ai::types::dialectic::{DialecticOptions, ReasoningLevel};
use honcho_ai::types::message::{MessageCreate, MessageSearchOptions};
use honcho_ai::types::session::{SessionContextOptions, SessionCreate};
use honcho_ai::{ConclusionCreateParams, FileSource, FinalResponse, Honcho};

#[test]
fn crate_constructs_client() {
    // Smoke: the primary entry point builds a usable client from valid inputs
    // and the workspace id round-trips through the public accessor.
    let client = Honcho::new("http://localhost:8000", "my-workspace")
        .expect("valid base url + workspace id should construct a client");
    assert_eq!(client.workspace_id(), "my-workspace");
}

#[test]
fn message_builder_works() {
    let msg = MessageCreate::builder()
        .content("hello world")
        .peer_id("peer-1")
        .build();
    assert_eq!(msg.content, "hello world");
    assert_eq!(msg.peer_id, "peer-1");
}

#[test]
fn session_create_builder_validates() {
    let valid = SessionCreate::builder().id("test-session").build();
    assert!(valid.validate().is_ok());

    let empty = SessionCreate::builder().id("").build();
    assert!(empty.validate().is_err());

    // Charset branch: anything outside `[a-zA-Z0-9_-]` is rejected.
    let bad_charset = SessionCreate::builder().id("bad id!").build();
    let err = bad_charset.validate().unwrap_err();
    assert!(
        err.to_string().contains("[a-zA-Z0-9_-]"),
        "expected a charset validation error, got: {err}"
    );
}

#[test]
fn dialectic_options_validate() {
    let opts = DialecticOptions::builder()
        .query("hello")
        .reasoning_level(ReasoningLevel::High)
        .build();
    assert!(opts.validate().is_ok());
    // The builder must actually carry the reasoning level through to the value.
    assert_eq!(opts.reasoning_level, ReasoningLevel::High);

    let empty = DialecticOptions::builder().query("").build();
    assert!(empty.validate().is_err());

    // Upper bound: queries longer than the 10_000-char limit are rejected.
    let too_long = DialecticOptions::builder()
        .query("x".repeat(10_001))
        .build();
    assert!(too_long.validate().is_err());
}

#[test]
fn session_context_options_defaults() {
    let opts = SessionContextOptions::builder().build();
    assert!(opts.summary);
    assert!(!opts.limit_to_session);
    // No cross-field constraints set → validation passes.
    assert!(opts.validate().is_ok());
}

#[test]
fn session_context_options_cross_field_validation() {
    // `peer_perspective` is meaningless without `peer_target`.
    let dangling_perspective = SessionContextOptions::builder()
        .peer_perspective("observer")
        .build();
    assert!(dangling_perspective.validate().is_err());

    // `search_query` likewise requires `peer_target`.
    let dangling_search = SessionContextOptions::builder()
        .search_query("topic")
        .build();
    assert!(dangling_search.validate().is_err());

    // With the companion field present, both constraints are satisfied.
    let ok = SessionContextOptions::builder()
        .peer_target("alice")
        .peer_perspective("observer")
        .search_query("topic")
        .build();
    assert!(ok.validate().is_ok());
}

#[test]
fn message_search_options_builder_default_limit() {
    // Distinct from the serde-default unit test in `src/types/message.rs`: this
    // asserts the *builder* default, not the deserialization default.
    let opts = MessageSearchOptions::builder().query("test").build();
    assert_eq!(opts.limit, 10);
}

#[test]
fn conclusion_create_params_new() {
    let params = ConclusionCreateParams::new("test conclusion");
    // Fields are `pub(crate)`, but the type derives `Serialize`, so assert the
    // wire shape: `content` is present and `session_id` (None) is omitted.
    let json = serde_json::to_value(&params).expect("ConclusionCreateParams serializes");
    assert_eq!(json["content"], "test conclusion");
    assert!(
        json.get("session_id").is_none(),
        "session_id must be omitted when None (skip_serializing_if)"
    );
    assert_eq!(json, serde_json::json!({ "content": "test conclusion" }));
}

#[test]
fn final_response_display() {
    let resp = FinalResponse::new("hello");
    assert_eq!(resp.content(), "hello");
    assert_eq!(resp.to_string(), "hello");
}

#[test]
fn file_source_bytes() {
    let src = FileSource::bytes("test.txt", b"hello", "text/plain");
    // Destructure the variant rather than asserting on the `Debug` string —
    // the latter is brittle and would silently pass on a truncated payload.
    match src {
        FileSource::Bytes {
            filename,
            bytes,
            content_type,
            ..
        } => {
            assert_eq!(filename, "test.txt");
            assert_eq!(bytes, b"hello");
            assert_eq!(content_type, "text/plain");
        }
        other => panic!("expected FileSource::Bytes, got {other:?}"),
    }
}
