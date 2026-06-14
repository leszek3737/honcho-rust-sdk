//! Integration tests for Session messages, delete, clone, and message ops (F6.4–F6.5).
//!
//! Chunking contract under test ([`Session::add_messages`]):
//! - empty input short-circuits with **zero** HTTP requests;
//! - `len <= 100` is a single POST;
//! - `len > 100` is split into batches of 100, each its own POST;
//! - a chunk that fails *after* an earlier chunk succeeded surfaces as
//!   [`HonchoError::PartialFailure`] carrying the already-created messages,
//!   whereas a failure on the very first chunk surfaces as the raw error.
//!
//! Chunk mocks are distinguished by `body_json` (chunk N matches exactly the
//! messages it should receive) rather than relying on wiremock mount order, so
//! request→chunk routing is deterministic. POST is non-idempotent, so a 5xx is
//! never retried and every chunk mock can assert `.expect(1)`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::needless_borrows_for_generic_args,
    clippy::unused_async,
    missing_docs
)]

mod common;

use std::collections::HashMap;
use std::ops::Range;

use common::{make_honcho, mount_workspace_ensure, page_json, session_response};
use honcho_ai::error::HonchoError;
use honcho_ai::session::Session;
use honcho_ai::types::message::MessageCreate;
use serde_json::{Value, json};
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Peer id used by every message fixture in this suite.
const PEER: &str = "alice";

/// Batch-create endpoint for the test session.
const MESSAGES_PATH: &str = "/v3/workspaces/ws1/sessions/sess1/messages";
/// Paginated list endpoint for the test session.
const MESSAGES_LIST_PATH: &str = "/v3/workspaces/ws1/sessions/sess1/messages/list";
/// The test session resource itself (delete / clone are mounted relative to it).
const SESSION_PATH: &str = "/v3/workspaces/ws1/sessions/sess1";

// ── Fixtures ───────────────────────────────────────────────────────────

/// One `MessageResponse` JSON body (the shape the server returns per message).
fn message_json(id: &str, content: &str, peer_id: &str) -> Value {
    json!({
        "id": id,
        "content": content,
        "peer_id": peer_id,
        "session_id": "sess1",
        "metadata": {},
        "created_at": "2025-01-15T10:30:00Z",
        "workspace_id": "ws1",
        "token_count": 2
    })
}

/// `SessionResponse` JSON for the clone endpoint (a *different* session id).
fn cloned_session_response() -> Value {
    json!({
        "id": "sess2",
        "is_active": true,
        "workspace_id": "ws1",
        "metadata": {},
        "configuration": {},
        "created_at": "2025-01-15T11:00:00Z"
    })
}

/// Builds `n` `MessageCreate`s with contents `msg0..msg{n-1}`, all from [`PEER`].
///
/// Factors out the message-building loop that was copy-pasted across the four
/// chunk tests.
fn build_msgs(n: usize) -> Vec<MessageCreate> {
    (0..n)
        .map(|i| {
            MessageCreate::builder()
                .content(format!("msg{i}"))
                .peer_id(PEER)
                .build()
        })
        .collect()
}

/// The exact wire body the SDK sends for the messages in `range`.
///
/// `MessageCreate` skips its `None` optionals, so each element is just
/// `{"content": .., "peer_id": ..}`. Used as a `body_json` matcher to pin a
/// mock to one specific chunk.
fn expected_batch_body(range: Range<usize>) -> Value {
    let messages: Vec<Value> = range
        .map(|i| json!({"content": format!("msg{i}"), "peer_id": PEER}))
        .collect();
    json!({ "messages": messages })
}

/// The server's array response for the messages in `range` (ids `m{i}`).
fn response_chunk(range: Range<usize>) -> Value {
    let items: Vec<Value> = range
        .map(|i| message_json(&format!("m{i}"), &format!("msg{i}"), PEER))
        .collect();
    Value::Array(items)
}

/// Mounts the workspace-ensure + session get-or-create mocks (each `.expect(1)`)
/// and returns a built [`Session`] for `sess1`.
async fn make_session(server: &MockServer) -> Session {
    mount_workspace_ensure(server, 1).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/sessions"))
        .and(body_json(&json!({"id": "sess1"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(session_response("sess1")))
        .expect(1)
        .mount(server)
        .await;

    let honcho = make_honcho(&server.uri());
    honcho.session("sess1").build().await.unwrap()
}

// ── F6.4: add_messages ─────────────────────────────────────────────────

#[tokio::test]
async fn add_messages_single_message() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    Mock::given(method("POST"))
        .and(path(MESSAGES_PATH))
        .and(body_json(expected_batch_body(0..1)))
        .respond_with(ResponseTemplate::new(200).set_body_json(response_chunk(0..1)))
        .expect(1)
        .mount(&server)
        .await;

    let result = session.add_messages(build_msgs(1)).await.unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].id(), "m0");
    assert_eq!(result[0].content(), "msg0");
    assert_eq!(result[0].peer_id(), PEER);
}

#[tokio::test]
async fn add_messages_preserves_duplicate_content_and_order() {
    // Two identical-content messages plus a distinct third: the SDK must send
    // all three verbatim, in order, with no dedup or reordering.
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    let body = json!({
        "messages": [
            {"content": "dup", "peer_id": PEER},
            {"content": "dup", "peer_id": PEER},
            {"content": "other", "peer_id": PEER}
        ]
    });

    Mock::given(method("POST"))
        .and(path(MESSAGES_PATH))
        .and(body_json(body))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            message_json("m0", "dup", PEER),
            message_json("m1", "dup", PEER),
            message_json("m2", "other", PEER)
        ])))
        .expect(1)
        .mount(&server)
        .await;

    let msgs = vec![
        MessageCreate::builder().content("dup").peer_id(PEER).build(),
        MessageCreate::builder().content("dup").peer_id(PEER).build(),
        MessageCreate::builder()
            .content("other")
            .peer_id(PEER)
            .build(),
    ];

    let result = session.add_messages(msgs).await.unwrap();
    assert_eq!(result.len(), 3);
    assert_eq!(result[0].content(), "dup");
    assert_eq!(result[1].content(), "dup");
    assert_eq!(result[2].content(), "other");
}

/// Drives the chunking boundary for every interesting size in one place.
///
/// `(total, expected_requests)`:
/// - `(3, 1)` / `(100, 1)`: at or below the 100-item limit → a single POST;
/// - `(101, 2)` / `(150, 2)`: above the limit → first 100, then the remainder.
///
/// Each chunk mock is pinned to its exact body via `body_json` and asserts
/// `.expect(1)`, so the request count is verified on server drop rather than
/// merely upper-bounded.
#[tokio::test]
async fn add_messages_chunks_at_hundred() {
    for (total, expected_requests) in [(3_usize, 1_u64), (100, 1), (101, 2), (150, 2)] {
        let server = MockServer::start().await;
        let session = make_session(&server).await;

        if expected_requests == 1 {
            Mock::given(method("POST"))
                .and(path(MESSAGES_PATH))
                .and(body_json(expected_batch_body(0..total)))
                .respond_with(ResponseTemplate::new(200).set_body_json(response_chunk(0..total)))
                .expect(1)
                .mount(&server)
                .await;
        } else {
            // chunk1 = first 100, chunk2 = the rest; distinguished by body_json.
            Mock::given(method("POST"))
                .and(path(MESSAGES_PATH))
                .and(body_json(expected_batch_body(0..100)))
                .respond_with(ResponseTemplate::new(200).set_body_json(response_chunk(0..100)))
                .expect(1)
                .mount(&server)
                .await;
            Mock::given(method("POST"))
                .and(path(MESSAGES_PATH))
                .and(body_json(expected_batch_body(100..total)))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(response_chunk(100..total)),
                )
                .expect(1)
                .mount(&server)
                .await;
        }

        let result = session.add_messages(build_msgs(total)).await.unwrap();
        assert_eq!(result.len(), total, "total={total}");
        assert_eq!(result[0].id(), "m0", "total={total}");
        assert_eq!(
            result[total - 1].id(),
            format!("m{}", total - 1),
            "total={total}"
        );
        // A mid-vector id confirms chunk responses are concatenated in order.
        assert_eq!(
            result[total / 2].id(),
            format!("m{}", total / 2),
            "total={total}"
        );
    }
}

#[tokio::test]
async fn add_messages_empty_is_ok_without_request() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    // Any POST here would be a bug: empty input must short-circuit before the
    // network. `.expect(0)` fails the test on server drop if a request arrives.
    Mock::given(method("POST"))
        .and(path(MESSAGES_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(0)
        .mount(&server)
        .await;

    let result = session.add_messages(vec![]).await.unwrap();
    assert!(result.is_empty());
}

#[tokio::test]
async fn add_messages_partial_failure_surfaces_sent_chunk() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    // 150 messages → chunk1 (first 100) succeeds, chunk2 (next 50) returns 500.
    // POST is non-idempotent, so the 500 is not retried → chunk2 is hit once.
    Mock::given(method("POST"))
        .and(path(MESSAGES_PATH))
        .and(body_json(expected_batch_body(0..100)))
        .respond_with(ResponseTemplate::new(200).set_body_json(response_chunk(0..100)))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(MESSAGES_PATH))
        .and(body_json(expected_batch_body(100..150)))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({"detail": "boom"})))
        .expect(1)
        .mount(&server)
        .await;

    let err = session.add_messages(build_msgs(150)).await.unwrap_err();

    // `sent == 100`: only the first chunk was persisted before the 500.
    assert!(
        matches!(err, HonchoError::PartialFailure { sent: 100, .. }),
        "expected PartialFailure {{ sent: 100 }}, got {err:?}"
    );
    let (messages, error) = err
        .into_partial_failure()
        .expect("error is a PartialFailure");
    assert_eq!(messages.len(), 100, "earlier-chunk messages are surfaced");
    assert_eq!(messages[0].id(), "m0");
    assert_eq!(messages[99].id(), "m99");
    // The boxed cause is the underlying 5xx server error.
    assert!(
        matches!(*error, HonchoError::Server { status: 500, .. }),
        "cause should be Server{{500}}, got {error:?}"
    );
}

#[tokio::test]
async fn add_messages_first_chunk_failure_is_clean_error() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    // The first (and only attempted) chunk fails. Because nothing was persisted
    // yet (`all.is_empty()`), the raw error is returned — never PartialFailure.
    Mock::given(method("POST"))
        .and(path(MESSAGES_PATH))
        .and(body_json(expected_batch_body(0..100)))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({"detail": "boom"})))
        .expect(1)
        .mount(&server)
        .await;
    // The second chunk must never be attempted once the first fails.
    Mock::given(method("POST"))
        .and(path(MESSAGES_PATH))
        .and(body_json(expected_batch_body(100..150)))
        .respond_with(ResponseTemplate::new(200).set_body_json(response_chunk(100..150)))
        .expect(0)
        .mount(&server)
        .await;

    let err = session.add_messages(build_msgs(150)).await.unwrap_err();
    assert!(
        !err.is_partial_failure(),
        "first-chunk failure must be a clean error, got {err:?}"
    );
    assert!(matches!(err, HonchoError::Server { status: 500, .. }));
}

// ── F6.4: messages (paginated) ─────────────────────────────────────────

#[tokio::test]
async fn messages_uses_default_pagination() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    let page_response = page_json(
        vec![
            message_json("msg1", "hello", PEER),
            message_json("msg2", "world", "bob"),
        ],
        2,
        1,
        50,
        1,
    );

    Mock::given(method("POST"))
        .and(path(MESSAGES_LIST_PATH))
        .and(query_param("page", "1"))
        .and(query_param("size", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page_response))
        .expect(1)
        .mount(&server)
        .await;

    let page = session.messages().await.unwrap();
    assert_eq!(page.total(), 2);
    assert_eq!(page.page(), 1);
    assert_eq!(page.size(), 50);
    assert_eq!(page.pages(), 1);
    let items = page.items();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].id(), "msg1");
    assert_eq!(items[1].id(), "msg2");
}

#[tokio::test]
async fn messages_with_options_sends_filters_body_and_query() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    let mut filters = HashMap::new();
    filters.insert("peer_id".to_string(), json!(PEER));

    Mock::given(method("POST"))
        .and(path(MESSAGES_LIST_PATH))
        .and(query_param("page", "2"))
        .and(query_param("size", "25"))
        .and(query_param("reverse", "true"))
        .and(body_json(json!({"peer_id": PEER})))
        .respond_with(ResponseTemplate::new(200).set_body_json(page_json(
            vec![message_json("msg1", "hello", PEER)],
            1,
            2,
            25,
            5,
        )))
        .expect(1)
        .mount(&server)
        .await;

    let page = session
        .messages_with_options(Some(filters), 2, 25, true)
        .await
        .unwrap();
    assert_eq!(page.page(), 2);
    assert_eq!(page.size(), 25);
    assert_eq!(page.pages(), 5);
    assert_eq!(page.items()[0].id(), "msg1");
}

// ── F6.5: delete ───────────────────────────────────────────────────────

#[tokio::test]
async fn delete_calls_delete_endpoint() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    Mock::given(method("DELETE"))
        .and(path(SESSION_PATH))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    session.delete().await.unwrap();
}

#[tokio::test]
async fn delete_not_found_maps_to_notfound() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    // 404 is not in the retryable set, so even idempotent DELETE is hit once.
    Mock::given(method("DELETE"))
        .and(path(SESSION_PATH))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"detail": "gone"})))
        .expect(1)
        .mount(&server)
        .await;

    let err = session.delete().await.unwrap_err();
    assert!(matches!(err, HonchoError::NotFound { .. }), "got {err:?}");
}

// ── F6.5: clone_session ───────────────────────────────────────────────

#[tokio::test]
async fn clone_session_returns_new_session() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/clone"))
        .respond_with(ResponseTemplate::new(200).set_body_json(cloned_session_response()))
        .expect(1)
        .mount(&server)
        .await;

    let cloned = session.clone_session().await.unwrap();
    assert_eq!(cloned.id(), "sess2");
    assert!(cloned.is_active());
}

#[tokio::test]
async fn clone_session_not_found_maps_to_notfound() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/clone"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"detail": "no session"})))
        .expect(1)
        .mount(&server)
        .await;

    // `clone_session` yields `Result<Session>`, and `Session` is not `Debug`,
    // so `unwrap_err` is unavailable — assert on the `Result` directly.
    let result = session.clone_session().await;
    assert!(
        matches!(result, Err(HonchoError::NotFound { .. })),
        "expected NotFound error"
    );
}

// ── F6.5: clone_session_with_message ───────────────────────────────────

#[tokio::test]
async fn clone_session_with_message_id_sends_query() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/clone"))
        .and(query_param("message_id", "msg42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(cloned_session_response()))
        .expect(1)
        .mount(&server)
        .await;

    let cloned = session.clone_session_with_message("msg42").await.unwrap();
    assert_eq!(cloned.id(), "sess2");
    assert!(cloned.is_active());
}

// ── F6.5: get_message ──────────────────────────────────────────────────

#[tokio::test]
async fn get_message_returns_message() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/messages/msg99"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(message_json("msg99", "found it", PEER)),
        )
        .expect(1)
        .mount(&server)
        .await;

    let msg = session.get_message("msg99").await.unwrap();
    assert_eq!(msg.id(), "msg99");
    assert_eq!(msg.content(), "found it");
}

#[tokio::test]
async fn get_message_not_found_maps_to_notfound() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/messages/missing"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"detail": "not found"})))
        .expect(1)
        .mount(&server)
        .await;

    let err = session.get_message("missing").await.unwrap_err();
    assert!(matches!(err, HonchoError::NotFound { .. }), "got {err:?}");
}

// ── F6.5: update_message ───────────────────────────────────────────────

#[tokio::test]
async fn update_message_puts_metadata() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    let updated_msg = json!({
        "id": "msg1",
        "content": "hello",
        "peer_id": PEER,
        "session_id": "sess1",
        "metadata": {"tagged": true},
        "created_at": "2025-01-15T10:30:00Z",
        "workspace_id": "ws1",
        "token_count": 2
    });

    Mock::given(method("PUT"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/messages/msg1"))
        .and(body_json(json!({"metadata": {"tagged": true}})))
        .respond_with(ResponseTemplate::new(200).set_body_json(updated_msg))
        .expect(1)
        .mount(&server)
        .await;

    let mut meta = HashMap::new();
    meta.insert("tagged".to_string(), json!(true));

    let msg = session.update_message("msg1", meta).await.unwrap();
    assert_eq!(msg.id(), "msg1");
    assert_eq!(msg.metadata().get("tagged").unwrap(), true);
}

#[tokio::test]
async fn update_message_not_found_maps_to_notfound() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    // 404 is not retryable, so the idempotent PUT is attempted exactly once.
    Mock::given(method("PUT"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/messages/msg1"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"detail": "no message"})))
        .expect(1)
        .mount(&server)
        .await;

    let mut meta = HashMap::new();
    meta.insert("tagged".to_string(), json!(true));

    let err = session.update_message("msg1", meta).await.unwrap_err();
    assert!(matches!(err, HonchoError::NotFound { .. }), "got {err:?}");
}
