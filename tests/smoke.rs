#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

use honcho_ai::types::dialectic::{DialecticOptions, ReasoningLevel};
use honcho_ai::types::message::{MessageCreate, MessageSearchOptions};
use honcho_ai::types::session::{SessionContextOptions, SessionCreate};
use honcho_ai::{ConclusionCreateParams, FileSource, FinalResponse, Message};

#[test]
fn crate_compiles() {
    // verify the crate compiles and key re-exports are accessible
    let _ = std::any::type_name::<honcho_ai::Honcho>();
    let _ = std::any::type_name::<honcho_ai::Peer>();
    let _ = std::any::type_name::<honcho_ai::Session>();
    let _ = std::any::type_name::<Message>();
    let _ = std::any::type_name::<honcho_ai::Conclusion>();
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
}

#[test]
fn dialectic_options_validate() {
    let opts = DialecticOptions::builder()
        .query("hello")
        .reasoning_level(ReasoningLevel::High)
        .build();
    assert!(opts.validate().is_ok());

    let empty = DialecticOptions::builder().query("").build();
    assert!(empty.validate().is_err());
}

#[test]
fn session_context_options_defaults() {
    let opts = SessionContextOptions::builder().build();
    assert!(opts.summary);
    assert!(!opts.limit_to_session);
}

#[test]
fn message_search_options_default_limit() {
    let opts = MessageSearchOptions::builder().query("test").build();
    assert_eq!(opts.limit, 10);
}

#[test]
fn conclusion_create_params_new() {
    let params = ConclusionCreateParams::new("test conclusion");
    // ConclusionCreateParams fields are pub(crate), so we can't access them
    // directly from external tests. Verify the constructor doesn't panic.
    let _ = params;
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
    // verify it's the bytes variant (Debug doesn't expose internals)
    let dbg = format!("{src:?}");
    assert!(dbg.contains("test.txt"));
    assert!(dbg.contains('5')); // byte count
}
