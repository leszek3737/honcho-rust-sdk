//! Validate + round-trip tests for message-related types.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

mod common;
use common::*;

use chrono::{DateTime, Utc};
use honcho_ai::types::message::*;
use serde_json::json;

/// Parses an RFC 3339 timestamp into a `DateTime<Utc>` for value asserts.
fn ts(s: &str) -> DateTime<Utc> {
    s.parse::<DateTime<Utc>>().unwrap()
}

macro_rules! schema_tests {
    ($type:ty, $schema:literal) => {
        paste::paste! {
            // Validate the SDK's *serialized output* (not the raw fixture) against
            // the OpenAPI schema, so a divergent Rust type (bad rename, missing
            // field) is actually exercised against the contract.
            #[test]
            fn [<validate_ $schema:snake _min>]() {
                let fixture = load_fixture($schema, "min");
                let value: $type = serde_json::from_value(fixture).unwrap();
                let output = serde_json::to_value(&value).unwrap();
                validate_openapi(&output, $schema);
            }

            #[test]
            fn [<validate_ $schema:snake _max>]() {
                let fixture = load_fixture($schema, "max");
                let value: $type = serde_json::from_value(fixture).unwrap();
                let output = serde_json::to_value(&value).unwrap();
                validate_openapi(&output, $schema);
            }

            // Strict fidelity (A0 keystone): SDK output must equal the input
            // fixture after canonicalization.
            #[test]
            fn [<roundtrip_ $schema:snake _min>]() {
                let fixture = load_fixture($schema, "min");
                roundtrip::<$type>(fixture);
            }

            #[test]
            fn [<roundtrip_ $schema:snake _max>]() {
                let fixture = load_fixture($schema, "max");
                roundtrip::<$type>(fixture);
            }
        }
    };
}

mod message_schemas {
    use super::*;

    schema_tests!(MessageResponse, "Message");
    schema_tests!(MessageCreate, "MessageCreate");
    schema_tests!(MessageBatchCreate, "MessageBatchCreate");
    schema_tests!(MessageUpdate, "MessageUpdate");
    schema_tests!(MessageConfiguration, "MessageConfiguration");

    // ------------------------------------------------------------------
    // MessageSearchOptions — handled outside `schema_tests!`
    //
    // `limit` is always serialized: it carries serde `default = default_limit`
    // with no `skip_serializing_if`, so the SDK injects `limit: 10` for the
    // `min` fixture, which deliberately omits it. A strict round-trip on `min`
    // would therefore fail on an intentional, documented addition (the
    // materialized wire default, twin of `ConclusionQuery::top_k`), so `min`
    // gets a dedicated golden-output test instead. `max` already carries
    // `limit`, so it round-trips strictly.
    // ------------------------------------------------------------------

    #[test]
    fn validate_message_search_options_min() {
        let fixture = load_fixture("MessageSearchOptions", "min");
        let value: MessageSearchOptions = serde_json::from_value(fixture).unwrap();
        let output = serde_json::to_value(&value).unwrap();
        validate_openapi(&output, "MessageSearchOptions");
    }

    #[test]
    fn validate_message_search_options_max() {
        let fixture = load_fixture("MessageSearchOptions", "max");
        let value: MessageSearchOptions = serde_json::from_value(fixture).unwrap();
        let output = serde_json::to_value(&value).unwrap();
        validate_openapi(&output, "MessageSearchOptions");
    }

    #[test]
    fn roundtrip_message_search_options_max() {
        let fixture = load_fixture("MessageSearchOptions", "max");
        roundtrip::<MessageSearchOptions>(fixture);
    }

    #[test]
    fn message_search_options_min_injects_default_limit() {
        // The `min` fixture omits `limit`; the SDK fills the documented default
        // and always emits it, so the wire output carries exactly one extra key.
        let opts: MessageSearchOptions =
            serde_json::from_value(load_fixture("MessageSearchOptions", "min")).unwrap();
        assert_eq!(
            opts.limit, 10,
            "absent limit must deserialize to the default"
        );
        assert!(opts.filters.is_none());

        let output = serde_json::to_value(&opts).unwrap();
        assert_eq!(
            output,
            json!({ "query": "test", "limit": 10 }),
            "min search options must serialize to query + injected default limit only"
        );
    }

    #[test]
    fn message_search_options_default_limit_is_indistinguishable_on_wire() {
        // `limit` has no skip predicate, so an *injected* default `10` and an
        // *explicit* `10` serialize identically — the materialized wire default,
        // which removes the absent-vs-default ambiguity.
        let injected: MessageSearchOptions =
            serde_json::from_value(json!({ "query": "q" })).unwrap();
        let explicit: MessageSearchOptions =
            serde_json::from_value(json!({ "query": "q", "limit": 10 })).unwrap();
        assert_eq!(
            serde_json::to_value(&injected).unwrap(),
            serde_json::to_value(&explicit).unwrap()
        );
    }
}

mod page_message_roundtrip {
    use super::*;

    #[test]
    fn roundtrip_page_message_min() {
        let fixture = load_fixture("Page_Message", "min");
        roundtrip::<MessagePage>(fixture);
    }

    #[test]
    fn roundtrip_page_message_max() {
        let fixture = load_fixture("Page_Message", "max");
        roundtrip::<MessagePage>(fixture);
    }
}

#[test]
fn message_all_fields_present() {
    let val = load_fixture("Message", "max");
    let msg: MessageResponse = serde_json::from_value(val).unwrap();
    assert_eq!(msg.id, "msg_02");
    assert_eq!(
        msg.content,
        "This is a longer message with details about the conversation and \
         context that spans multiple sentences for testing."
    );
    assert_eq!(msg.peer_id, "peer_abc123");
    assert_eq!(msg.session_id, "sess_xyz789");
    assert_eq!(msg.workspace_id, "ws_prod_001");
    assert_eq!(msg.token_count, 127);
    assert_eq!(msg.created_at, ts("2025-06-15T12:30:45.123456Z"));
    // Assert nested metadata values, not merely key presence.
    assert_eq!(msg.metadata["key"], json!("value"));
    assert_eq!(msg.metadata["nested"]["inner"], json!(true));
    assert_eq!(msg.metadata["count"], json!(42));
}

#[test]
fn message_create_optional_fields_none() {
    let val = load_fixture("MessageCreate", "min");
    let mc: MessageCreate = serde_json::from_value(val).unwrap();
    assert_eq!(mc.content, "hello");
    assert_eq!(mc.peer_id, "peer_01");
    assert!(mc.metadata.is_none());
    assert!(mc.configuration.is_none());
    assert!(mc.created_at.is_none());
}

#[test]
fn message_batch_create_elements() {
    let val = load_fixture("MessageBatchCreate", "max");
    let batch: MessageBatchCreate = serde_json::from_value(val).unwrap();
    assert_eq!(batch.messages.len(), 2);

    // Assert the 2nd element carries its nested configuration + created_at,
    // not just that the vector has two entries.
    let second = &batch.messages[1];
    assert_eq!(second.content, "Hello from Bob");
    assert_eq!(second.peer_id, "peer_bob");
    let reasoning = second
        .configuration
        .as_ref()
        .unwrap()
        .reasoning
        .as_ref()
        .unwrap();
    assert_eq!(reasoning.enabled, Some(true));
    assert_eq!(second.created_at, Some(ts("2025-06-01T08:00:00Z")));
}

#[test]
fn message_update_empty_is_valid() {
    let val = load_fixture("MessageUpdate", "min");
    let upd: MessageUpdate = serde_json::from_value(val).unwrap();
    assert!(upd.metadata.is_none());
}

#[test]
fn message_configuration_with_reasoning() {
    let val = load_fixture("MessageConfiguration", "max");
    let cfg: MessageConfiguration = serde_json::from_value(val).unwrap();
    let r = cfg.reasoning.unwrap();
    assert_eq!(r.enabled, Some(true));
    assert_eq!(
        r.custom_instructions,
        Some("Analyze sentiment carefully".to_string())
    );
}

#[test]
fn message_page_empty() {
    let val = load_fixture("Page_Message", "min");
    let page: MessagePage = serde_json::from_value(val).unwrap();
    // `items_ref()` returns a borrowed slice; `items()` would clone every item.
    assert!(page.items_ref().is_empty());
    assert_eq!(page.total(), 0);
    assert_eq!(page.page(), 1);
    assert_eq!(page.size(), 20);
    assert_eq!(page.pages(), 0);
}

#[test]
fn message_page_with_items() {
    let val = load_fixture("Page_Message", "max");
    let page: MessagePage = serde_json::from_value(val).unwrap();
    let items = page.items_ref();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].id, "msg_01");
    assert_eq!(items[1].id, "msg_02");
    assert_eq!(items[1].content, "second message");
    assert_eq!(page.total(), 42);
    assert_eq!(page.page(), 2);
    assert_eq!(page.size(), 10);
    assert_eq!(page.pages(), 5);
}

/// Negative and invariant coverage.
///
/// Two classes here:
/// * Missing-required / wrong-type cases are *enforced by serde* → `Err`.
/// * Range invariants (`limit` 1..=100, `content` <= 25 000, batch 1..=100)
///   are **server-validated only**; the SDK models plain `String`/`Vec`/`u32`
///   with no client-side bounds, so deserialization is intentionally lenient.
///   Those tests assert today's behavior and act as gates: if PR5/PR6 ever
///   add client-side range validation, they flip from accepting to rejecting.
mod negative_and_invariant {
    use super::*;

    #[test]
    fn message_create_missing_content_is_err() {
        let val = json!({ "peer_id": "peer_01" });
        let e = serde_json::from_value::<MessageCreate>(val).unwrap_err();
        assert!(e.to_string().contains("missing field"), "{e}");
    }

    #[test]
    fn message_create_missing_peer_id_is_err() {
        let val = json!({ "content": "hi" });
        let e = serde_json::from_value::<MessageCreate>(val).unwrap_err();
        assert!(e.to_string().contains("missing field"), "{e}");
    }

    #[test]
    fn message_create_wrong_type_content_is_err() {
        let val = json!({ "content": 123, "peer_id": "peer_01" });
        let e = serde_json::from_value::<MessageCreate>(val).unwrap_err();
        assert!(e.to_string().contains("invalid type"), "{e}");
    }

    #[test]
    fn message_response_missing_created_at_is_err() {
        // `created_at` has no `serde(default)` → its absence must error.
        let val = json!({
            "id": "m1",
            "content": "hi",
            "peer_id": "p1",
            "session_id": "s1",
            "workspace_id": "w1"
        });
        let e = serde_json::from_value::<MessageResponse>(val).unwrap_err();
        assert!(e.to_string().contains("missing field"), "{e}");
    }

    #[test]
    fn message_batch_create_missing_messages_is_err() {
        let val = json!({});
        let e = serde_json::from_value::<MessageBatchCreate>(val).unwrap_err();
        assert!(e.to_string().contains("missing field"), "{e}");
    }

    #[test]
    fn message_search_limit_out_of_range_is_accepted() {
        // Server contract: 1..=100. SDK does not validate the bound.
        let val = json!({ "query": "q", "limit": 100_000 });
        let opts: MessageSearchOptions = serde_json::from_value(val).unwrap();
        assert_eq!(opts.limit, 100_000);
    }

    #[test]
    fn message_batch_empty_is_accepted() {
        // Server contract: 1..=100 messages. SDK models a plain `Vec`.
        let val = json!({ "messages": [] });
        let batch: MessageBatchCreate = serde_json::from_value(val).unwrap();
        assert!(batch.messages.is_empty());
    }

    #[test]
    fn message_create_oversized_content_is_accepted() {
        // Server contract: content <= 25 000 chars. SDK does not check length.
        let content = "x".repeat(25_001);
        let val = json!({ "content": content, "peer_id": "p1" });
        let mc: MessageCreate = serde_json::from_value(val).unwrap();
        assert_eq!(mc.content.len(), 25_001);
    }
}
