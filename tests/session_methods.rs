//! Integration tests for Session context, summaries, search, representation, and `queue_status`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::needless_borrows_for_generic_args,
    missing_docs
)]

use std::collections::HashMap;

use honcho_ai::error::HonchoError;
use honcho_ai::session::Session;
use honcho_ai::types::session::SessionContextOptions;
use serde_json::{Value, json};
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

mod common;

fn workspace_response_json() -> Value {
    json!({
        "id": "ws1",
        "metadata": {},
        "configuration": {},
        "created_at": "2025-01-15T10:30:00Z"
    })
}

fn session_response_json() -> Value {
    json!({
        "id": "sess1",
        "workspace_id": "ws1",
        "is_active": true,
        "metadata": {},
        "configuration": {},
        "created_at": "2025-01-15T10:30:00Z"
    })
}

/// Mounts the workspace-ensure + session get-or-create POSTs that every
/// `Session::build()` triggers. Both are one-shot (`up_to_n_times(1)`) so the
/// per-test request inspection below only ever sees the call under test plus
/// these two setup requests.
async fn mount_session_setup(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/v3/workspaces"))
        .and(body_json(json!({"id": "ws1"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(workspace_response_json()))
        .up_to_n_times(1)
        .mount(server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/sessions"))
        .and(body_json(json!({"id": "sess1"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(session_response_json()))
        .up_to_n_times(1)
        .mount(server)
        .await;
}

/// Builds a `Session` against `server` using the default retry policy.
async fn make_session(server: &MockServer) -> Session {
    mount_session_setup(server).await;
    let honcho = common::make_honcho(&server.uri());
    honcho.session("sess1").build().await.unwrap()
}

/// Builds a `Session` whose client never retries (`max_retries(0)`).
///
/// Used by the 5xx error-path tests: a retryable `500` on an idempotent `GET`
/// would otherwise be re-sent by the default policy, so disabling retries keeps
/// those tests to exactly one request and lets the mock assert `.expect(1)`.
async fn make_session_no_retry(server: &MockServer) -> Session {
    mount_session_setup(server).await;
    let honcho = common::make_honcho_no_retry(&server.uri());
    honcho.session("sess1").build().await.unwrap()
}

/// Returns the query params of the single recorded request whose path ends with
/// `suffix`, as an owned `key -> value` map.
///
/// Unlike wiremock's positive-only `query_param` matcher, the returned map lets
/// a test assert the *exact* param set (via `.len()` and key absence), so an
/// unexpected extra param is caught rather than silently tolerated.
async fn single_request_query(server: &MockServer, suffix: &str) -> HashMap<String, String> {
    let requests = server
        .received_requests()
        .await
        .expect("request recording is enabled by default");
    let matching: Vec<&Request> = requests
        .iter()
        .filter(|r| r.url.path().ends_with(suffix))
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one request to a path ending with {suffix}"
    );
    matching[0]
        .url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

/// Asserts that no request was sent to a path ending with `suffix`.
async fn assert_no_request_to(server: &MockServer, suffix: &str) {
    let requests = server
        .received_requests()
        .await
        .expect("request recording is enabled by default");
    assert!(
        requests.iter().all(|r| !r.url.path().ends_with(suffix)),
        "expected no request to a path ending with {suffix}"
    );
}

fn context_response_json() -> Value {
    json!({
        "id": "sess1",
        "messages": [
            {
                "id": "m1",
                "content": "hello",
                "peer_id": "user1",
                "session_id": "sess1",
                "metadata": {},
                "created_at": "2025-01-15T10:30:00Z",
                "workspace_id": "ws1",
                "token_count": 1
            }
        ],
        "summary": {
            "content": "a summary",
            "message_id": "msg0",
            "summary_type": "short",
            "created_at": "2025-01-15T10:30:00Z",
            "token_count": 5
        },
        "peer_representation": "some rep",
        "peer_card": ["fact1"]
    })
}

fn summary_json(content: &str, summary_type: &str, token_count: u32) -> Value {
    json!({
        "content": content,
        "message_id": "msg0",
        "summary_type": summary_type,
        "created_at": "2025-01-15T10:30:00Z",
        "token_count": token_count
    })
}

fn search_message_json(id: &str, content: &str) -> Value {
    json!({
        "id": id,
        "content": content,
        "peer_id": "user1",
        "session_id": "sess1",
        "metadata": {},
        "created_at": "2025-01-15T10:30:00Z",
        "workspace_id": "ws1",
        "token_count": 2
    })
}

// ── F6.6: Context ────────────────────────────────────────────────────

#[tokio::test]
async fn session_context_returns_session_context() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/context"))
        .and(query_param("summary", "true"))
        .and(query_param("limit_to_session", "false"))
        .respond_with(ResponseTemplate::new(200).set_body_json(context_response_json()))
        .expect(1)
        .mount(&server)
        .await;

    let ctx = session.context().await.unwrap();
    assert_eq!(ctx.id, "sess1");
    assert_eq!(ctx.messages.len(), 1);
    assert_eq!(ctx.messages[0].content, "hello");
    assert!(ctx.summary.is_some());
    assert_eq!(ctx.summary.unwrap().content, "a summary");
    assert_eq!(ctx.peer_representation.as_deref(), Some("some rep"));
    assert_eq!(ctx.peer_card, Some(vec!["fact1".to_string()]));
}

#[tokio::test]
async fn session_context_not_found() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/context"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"detail": "no session"})))
        .expect(1)
        .mount(&server)
        .await;

    let err = session.context().await.unwrap_err();
    assert_eq!(err.status_code(), Some(404));
    assert!(matches!(err, HonchoError::NotFound { .. }));
}

#[tokio::test]
async fn session_context_server_error() {
    let server = MockServer::start().await;
    let session = make_session_no_retry(&server).await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/context"))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({"detail": "boom"})))
        .expect(1)
        .mount(&server)
        .await;

    let err = session.context().await.unwrap_err();
    assert_eq!(err.status_code(), Some(500));
    assert!(matches!(err, HonchoError::Server { status: 500, .. }));
}

// ── F6.8: Summaries ──────────────────────────────────────────────────

#[tokio::test]
async fn session_summaries_returns_both() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/summaries"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "sess1",
            "short_summary": summary_json("short one", "short", 3),
            "long_summary": summary_json("long one", "long", 10)
        })))
        .expect(1)
        .mount(&server)
        .await;

    let summaries = session.summaries().await.unwrap();
    assert_eq!(summaries.id, "sess1");
    assert!(summaries.short_summary.is_some());
    assert_eq!(summaries.short_summary.unwrap().content, "short one");
    assert!(summaries.long_summary.is_some());
    assert_eq!(summaries.long_summary.unwrap().content, "long one");
}

#[tokio::test]
async fn session_summaries_none_when_not_available() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/summaries"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "sess1"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let summaries = session.summaries().await.unwrap();
    assert_eq!(summaries.id, "sess1");
    assert!(summaries.short_summary.is_none());
    assert!(summaries.long_summary.is_none());
}

#[tokio::test]
async fn session_summaries_short_only() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/summaries"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "sess1",
            "short_summary": summary_json("short one", "short", 3)
        })))
        .expect(1)
        .mount(&server)
        .await;

    let summaries = session.summaries().await.unwrap();
    assert_eq!(
        summaries.short_summary.as_ref().map(|s| s.content.as_str()),
        Some("short one")
    );
    assert!(summaries.long_summary.is_none());
}

#[tokio::test]
async fn session_summaries_long_only() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/summaries"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "sess1",
            "long_summary": summary_json("long one", "long", 10)
        })))
        .expect(1)
        .mount(&server)
        .await;

    let summaries = session.summaries().await.unwrap();
    assert!(summaries.short_summary.is_none());
    assert_eq!(
        summaries.long_summary.as_ref().map(|s| s.content.as_str()),
        Some("long one")
    );
}

#[tokio::test]
async fn session_summaries_not_found() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/summaries"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"detail": "no session"})))
        .expect(1)
        .mount(&server)
        .await;

    let err = session.summaries().await.unwrap_err();
    assert_eq!(err.status_code(), Some(404));
    assert!(matches!(err, HonchoError::NotFound { .. }));
}

#[tokio::test]
async fn session_summaries_server_error() {
    let server = MockServer::start().await;
    let session = make_session_no_retry(&server).await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/summaries"))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({"detail": "boom"})))
        .expect(1)
        .mount(&server)
        .await;

    let err = session.summaries().await.unwrap_err();
    assert_eq!(err.status_code(), Some(500));
    assert!(matches!(err, HonchoError::Server { status: 500, .. }));
}

// ── F6.9: Search ─────────────────────────────────────────────────────

#[tokio::test]
async fn session_search_returns_messages() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/search"))
        .and(body_json(json!({
            "query": "hello",
            "limit": 10
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!([search_message_json("m1", "hello world")])),
        )
        .expect(1)
        .mount(&server)
        .await;

    let results = session.search("hello").await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].content(), "hello world");
}

#[tokio::test]
async fn session_search_returns_empty() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/search"))
        .and(body_json(json!({"query": "nothing", "limit": 10})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&server)
        .await;

    let results = session.search("nothing").await.unwrap();
    assert!(results.is_empty());
}

#[tokio::test]
async fn session_search_returns_multiple() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/search"))
        .and(body_json(json!({"query": "hello", "limit": 10})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            search_message_json("m1", "hello world"),
            search_message_json("m2", "hello again")
        ])))
        .expect(1)
        .mount(&server)
        .await;

    let results = session.search("hello").await.unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].content(), "hello world");
    assert_eq!(results[1].content(), "hello again");
}

#[tokio::test]
async fn session_search_validates_empty_query() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    let err = session.search("").await.unwrap_err();
    assert_eq!(err.code(), "validation_error");
    assert!(matches!(err, HonchoError::Validation(_)));

    // The empty-query check must short-circuit before any network call: assert
    // no `/search` POST ever reached the server. Without this, the test would
    // still pass if validation regressed, because an un-mocked POST yields a
    // *different* error (wiremock's default 404 -> `not_found`) rather than the
    // `validation_error` the `code()` check happens to also reject.
    assert_no_request_to(&server, "/search").await;
}

#[tokio::test]
async fn session_search_not_found() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/search"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"detail": "no session"})))
        .expect(1)
        .mount(&server)
        .await;

    let err = session.search("hello").await.unwrap_err();
    assert_eq!(err.status_code(), Some(404));
    assert!(matches!(err, HonchoError::NotFound { .. }));
}

#[tokio::test]
async fn session_search_server_error() {
    let server = MockServer::start().await;
    let session = make_session_no_retry(&server).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/search"))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({"detail": "boom"})))
        .expect(1)
        .mount(&server)
        .await;

    let err = session.search("hello").await.unwrap_err();
    assert_eq!(err.status_code(), Some(500));
    assert!(matches!(err, HonchoError::Server { status: 500, .. }));
}

// ── F6.9: Representation ──────────────────────────────────────────────

#[tokio::test]
async fn session_representation_posts_to_peer_representation() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/alice/representation"))
        .and(body_json(json!({"session_id": "sess1"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "representation": "Alice likes Rust"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let rep = session.representation("alice").await.unwrap();
    assert_eq!(rep, "Alice likes Rust");
}

#[tokio::test]
async fn session_representation_validates_search_params() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    // `search_top_k = 0` is below the valid range (1..=100); the builder must
    // reject it locally before issuing any request.
    let err = session
        .representation_builder("alice")
        .search_top_k(0)
        .send()
        .await
        .unwrap_err();
    assert_eq!(err.status_code(), None);
    assert!(matches!(err, HonchoError::Validation(_)));

    assert_no_request_to(&server, "/representation").await;
}

#[tokio::test]
async fn session_representation_not_found() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/alice/representation"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"detail": "no peer"})))
        .expect(1)
        .mount(&server)
        .await;

    let err = session.representation("alice").await.unwrap_err();
    assert_eq!(err.status_code(), Some(404));
    assert!(matches!(err, HonchoError::NotFound { .. }));
}

// ── F6.9: Queue Status ────────────────────────────────────────────────

#[tokio::test]
async fn session_queue_status_gets_with_session_id() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/queue/status"))
        .and(query_param("session_id", "sess1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "total_work_units": 5,
            "completed_work_units": 3,
            "in_progress_work_units": 1,
            "pending_work_units": 1
        })))
        .expect(1)
        .mount(&server)
        .await;

    let status = session.queue_status(None, None).await.unwrap();
    assert_eq!(status.total_work_units, 5);
    assert_eq!(status.completed_work_units, 3);
    assert_eq!(status.in_progress_work_units, 1);
    assert_eq!(status.pending_work_units, 1);

    // With both args `None`, only `session_id` must be on the wire: assert the
    // exact param set so a spurious `observer_id`/`sender_id` would be caught.
    let q = single_request_query(&server, "/status").await;
    assert_eq!(q.get("session_id").map(String::as_str), Some("sess1"));
    assert!(!q.contains_key("observer_id"));
    assert!(!q.contains_key("sender_id"));
    assert_eq!(q.len(), 1);
}

#[tokio::test]
async fn session_queue_status_includes_observer_and_sender() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/queue/status"))
        .and(query_param("session_id", "sess1"))
        .and(query_param("observer_id", "obs1"))
        .and(query_param("sender_id", "snd1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "total_work_units": 0,
            "completed_work_units": 0,
            "in_progress_work_units": 0,
            "pending_work_units": 0
        })))
        .expect(1)
        .mount(&server)
        .await;

    let status = session
        .queue_status(Some("obs1"), Some("snd1"))
        .await
        .unwrap();
    assert_eq!(status.total_work_units, 0);

    let q = single_request_query(&server, "/status").await;
    assert_eq!(q.get("session_id").map(String::as_str), Some("sess1"));
    assert_eq!(q.get("observer_id").map(String::as_str), Some("obs1"));
    assert_eq!(q.get("sender_id").map(String::as_str), Some("snd1"));
    assert_eq!(q.len(), 3);
}

#[tokio::test]
async fn session_queue_status_not_found() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/queue/status"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"detail": "no queue"})))
        .expect(1)
        .mount(&server)
        .await;

    let err = session.queue_status(None, None).await.unwrap_err();
    assert_eq!(err.status_code(), Some(404));
    assert!(matches!(err, HonchoError::NotFound { .. }));
}

#[tokio::test]
async fn session_queue_status_server_error() {
    let server = MockServer::start().await;
    let session = make_session_no_retry(&server).await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/queue/status"))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({"detail": "boom"})))
        .expect(1)
        .mount(&server)
        .await;

    let err = session.queue_status(None, None).await.unwrap_err();
    assert_eq!(err.status_code(), Some(500));
    assert!(matches!(err, HonchoError::Server { status: 500, .. }));
}

// ── Context with options ───────────────────────────────────────────

#[tokio::test]
async fn session_context_with_options_sends_all_query_params() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    // Match only method + path; the exact query set is asserted from the
    // recorded request below so numeric/float formatting is checked by parsing
    // rather than by a brittle exact-string `query_param` matcher.
    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/context"))
        .respond_with(ResponseTemplate::new(200).set_body_json(context_response_json()))
        .expect(1)
        .mount(&server)
        .await;

    let opts = SessionContextOptions::builder()
        .summary(false)
        .limit_to_session(true)
        .tokens(4096)
        .peer_target("bob")
        .peer_perspective("alice")
        .search_query("preferences")
        .search_top_k(10)
        .search_max_distance(0.5)
        .include_most_frequent(true)
        .max_conclusions(20)
        .build();

    let ctx = session.context_with_options(&opts).await.unwrap();
    assert_eq!(ctx.id, "sess1");
    assert_eq!(ctx.messages.len(), 1);
    assert_eq!(ctx.peer_representation.as_deref(), Some("some rep"));

    let q = single_request_query(&server, "/context").await;
    assert_eq!(q.get("summary").map(String::as_str), Some("false"));
    assert_eq!(q.get("limit_to_session").map(String::as_str), Some("true"));
    assert_eq!(q.get("tokens").map(String::as_str), Some("4096"));
    assert_eq!(q.get("peer_target").map(String::as_str), Some("bob"));
    assert_eq!(q.get("peer_perspective").map(String::as_str), Some("alice"));
    assert_eq!(
        q.get("search_query").map(String::as_str),
        Some("preferences")
    );
    assert_eq!(q.get("search_top_k").map(String::as_str), Some("10"));
    assert_eq!(
        q.get("include_most_frequent").map(String::as_str),
        Some("true")
    );
    assert_eq!(q.get("max_conclusions").map(String::as_str), Some("20"));
    // Float: assert the parsed value, not its string spelling.
    let dist: f64 = q.get("search_max_distance").unwrap().parse().unwrap();
    assert!((dist - 0.5).abs() < 1e-9);
    // Exactly these 10 params; no extras.
    assert_eq!(q.len(), 10);
}

#[tokio::test]
async fn session_context_with_options_sends_only_set_params() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/context"))
        .respond_with(ResponseTemplate::new(200).set_body_json(context_response_json()))
        .expect(1)
        .mount(&server)
        .await;

    let opts = SessionContextOptions::builder()
        .summary(true)
        .limit_to_session(false)
        .build();

    let ctx = session.context_with_options(&opts).await.unwrap();
    assert_eq!(ctx.id, "sess1");

    // Core fix: assert the EXACT query set. The old `query_param` matchers only
    // checked presence, so an added `tokens`/`peer_target`/... would still pass.
    // A full map with a `.len()` assertion rejects any extra param.
    let q = single_request_query(&server, "/context").await;
    assert_eq!(q.get("summary").map(String::as_str), Some("true"));
    assert_eq!(q.get("limit_to_session").map(String::as_str), Some("false"));
    assert_eq!(q.len(), 2);
}

// ── T3.4: Cross-field validation ────────────────────────────────────

#[test]
fn session_context_options_peer_perspective_requires_peer_target() {
    let opts = SessionContextOptions::builder()
        .peer_perspective("alice")
        .build();
    let err = opts.validate().unwrap_err();
    assert_eq!(err.code(), "validation_error");
}

#[test]
fn session_context_options_both_set_succeeds() {
    let opts = SessionContextOptions::builder()
        .peer_perspective("alice")
        .peer_target("bob")
        .build();
    assert_eq!(opts.peer_perspective.as_deref(), Some("alice"));
    assert_eq!(opts.peer_target.as_deref(), Some("bob"));
}

#[test]
fn session_context_options_no_perspective_no_target_succeeds() {
    let opts = SessionContextOptions::builder().build();
    assert!(opts.peer_perspective.is_none());
    assert!(opts.peer_target.is_none());
}
