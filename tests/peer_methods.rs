//! Wiremock tests for Peer methods (F5.5–F5.7) and streaming chat (F8.4).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::needless_borrows_for_generic_args,
    clippy::unused_async,
    missing_docs
)]

use futures_util::StreamExt;
use serde_json::json;
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use honcho_ai::Peer;
use honcho_ai::error::HonchoError;

mod common;

/// Boots a mock server, mounts the workspace-ensure + peer-create flow, then
/// drives `honcho.peer("alice").build()` and hands back the live `Peer`.
///
/// Collapses the construction boilerplate that was previously copy-pasted into
/// every test (×13). Both setup mocks carry an exact `.expect(1)` so wiremock
/// verifies on server drop that the build issued exactly one of each. The
/// `Honcho` handle is dropped on return: a `Peer` owns clones of the HTTP
/// client / workspace id / peer id, so it stays valid and never re-issues the
/// (cached) workspace-ensure for subsequent calls.
async fn setup() -> (MockServer, Peer) {
    let server = MockServer::start().await;
    let honcho = common::make_honcho(&server.uri());

    common::mount_workspace_ensure(&server, 1).await;
    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers"))
        .and(body_json(json!({ "id": "alice" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(common::peer_response("alice")))
        .expect(1)
        .mount(&server)
        .await;

    let peer = honcho.peer("alice").build().await.unwrap();
    (server, peer)
}

// ── F5.5 Representation ────────────────────────────────────────────

#[tokio::test]
async fn peer_representation_basic() {
    let (server, peer) = setup().await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/alice/representation"))
        .and(body_json(json!({})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "representation": "Alice likes cats and Rust."
        })))
        .expect(1)
        .mount(&server)
        .await;

    let repr = peer.representation().await.unwrap();
    assert_eq!(repr, "Alice likes cats and Rust.");
}

#[tokio::test]
async fn peer_representation_with_options() {
    let (server, peer) = setup().await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/alice/representation"))
        .and(body_json(json!({
            "session_id": "sess1",
            "target": "bob",
            "search_query": "preferences",
            "search_top_k": 5,
            "search_max_distance": 0.8,
            "include_most_frequent": true,
            "max_conclusions": 20
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "representation": "curated result"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let repr = peer
        .representation_builder()
        .session_id("sess1")
        .target("bob")
        .search_query("preferences")
        .search_top_k(5)
        .search_max_distance(0.8)
        .include_most_frequent(true)
        .max_conclusions(20)
        .send()
        .await
        .unwrap();
    assert_eq!(repr, "curated result");
}

/// One out-of-range search/conclusion parameter, used to table-drive the
/// `RepresentationBuilder` client-side validation cases.
#[derive(Clone, Copy)]
enum BadSearchParam {
    SearchTopK(u32),
    SearchMaxDistance(f64),
    MaxConclusions(u32),
}

#[tokio::test]
async fn peer_representation_rejects_out_of_range_params() {
    let (server, peer) = setup().await;

    // Each invalid value must yield `Validation` mentioning the offending field.
    let cases = [
        (BadSearchParam::SearchTopK(0), "search_top_k"),
        (BadSearchParam::SearchTopK(101), "search_top_k"),
        (
            BadSearchParam::SearchMaxDistance(1.5),
            "search_max_distance",
        ),
        (
            BadSearchParam::SearchMaxDistance(-0.1),
            "search_max_distance",
        ),
        (BadSearchParam::MaxConclusions(0), "max_conclusions"),
        (BadSearchParam::MaxConclusions(101), "max_conclusions"),
    ];

    for (param, field) in cases {
        let builder = peer.representation_builder();
        let builder = match param {
            BadSearchParam::SearchTopK(v) => builder.search_top_k(v),
            BadSearchParam::SearchMaxDistance(v) => builder.search_max_distance(v),
            BadSearchParam::MaxConclusions(v) => builder.max_conclusions(v),
        };
        let err = builder.send().await.unwrap_err();
        assert!(
            matches!(err, HonchoError::Validation(ref msg) if msg.contains(field)),
            "{field}: expected Validation error, got {err:?}"
        );
    }

    // Validation runs client-side: no representation request ever left the SDK.
    // (Only the two setup POSTs were recorded.)
    let requests = server.received_requests().await.unwrap();
    assert!(
        !requests
            .iter()
            .any(|r| r.url.path().ends_with("/representation")),
        "out-of-range params must short-circuit before any representation request"
    );
}

#[tokio::test]
async fn peer_representation_accepts_valid_boundaries() {
    let (server, peer) = setup().await;

    // Lower inclusive bounds: top_k=1, max_distance=0.0, max_conclusions=1.
    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/alice/representation"))
        .and(body_json(json!({
            "search_top_k": 1,
            "search_max_distance": 0.0,
            "max_conclusions": 1
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "representation": "low bounds"
        })))
        .expect(1)
        .mount(&server)
        .await;

    // Upper inclusive bounds: top_k=100, max_distance=1.0, max_conclusions=100.
    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/alice/representation"))
        .and(body_json(json!({
            "search_top_k": 100,
            "search_max_distance": 1.0,
            "max_conclusions": 100
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "representation": "high bounds"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let low = peer
        .representation_builder()
        .search_top_k(1)
        .search_max_distance(0.0)
        .max_conclusions(1)
        .send()
        .await
        .unwrap();
    assert_eq!(low, "low bounds");

    let high = peer
        .representation_builder()
        .search_top_k(100)
        .search_max_distance(1.0)
        .max_conclusions(100)
        .send()
        .await
        .unwrap();
    assert_eq!(high, "high bounds");
}

// ── F5.6 Context ───────────────────────────────────────────────────

#[tokio::test]
async fn peer_context_returns_peer_context() {
    let (server, peer) = setup().await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/peers/alice/context"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "peer_id": "alice",
            "target_id": "alice",
            "representation": "Alice is curious.",
            "peer_card": ["friendly", "inquisitive"]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let ctx = peer.context().await.unwrap();
    assert_eq!(ctx.peer_id, "alice");
    assert_eq!(ctx.target_id, "alice");
    assert_eq!(ctx.representation.as_deref(), Some("Alice is curious."));
    assert_eq!(ctx.peer_card.unwrap(), ["friendly", "inquisitive"]);
}

#[tokio::test]
async fn peer_context_with_target_sends_query() {
    let (server, peer) = setup().await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/peers/alice/context"))
        .and(query_param("target", "bob"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "peer_id": "alice",
            "target_id": "bob",
            "representation": "Bob is helpful.",
            "peer_card": null
        })))
        .expect(1)
        .mount(&server)
        .await;

    let ctx = peer.context_builder().target("bob").send().await.unwrap();
    assert_eq!(ctx.peer_id, "alice");
    assert_eq!(ctx.target_id, "bob");
    assert_eq!(ctx.representation.as_deref(), Some("Bob is helpful."));
    assert!(ctx.peer_card.is_none());
}

#[tokio::test]
async fn peer_context_with_options_sends_all_query_params() {
    let (server, peer) = setup().await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/peers/alice/context"))
        .and(query_param("target", "bob"))
        .and(query_param("search_query", "preferences"))
        .and(query_param("search_top_k", "10"))
        .and(query_param("search_max_distance", "0.5"))
        .and(query_param("include_most_frequent", "true"))
        .and(query_param("max_conclusions", "20"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "peer_id": "alice",
            "target_id": "bob",
            "representation": "curated context",
            "peer_card": null
        })))
        .expect(1)
        .mount(&server)
        .await;

    let ctx = peer
        .context_builder()
        .target("bob")
        .search_query("preferences")
        .search_top_k(10)
        .search_max_distance(0.5)
        .include_most_frequent(true)
        .max_conclusions(20)
        .send()
        .await
        .unwrap();
    assert_eq!(ctx.peer_id, "alice");
    assert_eq!(ctx.target_id, "bob");
    assert_eq!(ctx.representation.as_deref(), Some("curated context"));
}

#[tokio::test]
async fn peer_context_with_options_sends_only_set_params() {
    let (server, peer) = setup().await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/peers/alice/context"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "peer_id": "alice",
            "target_id": "alice",
            "representation": "self context",
            "peer_card": null
        })))
        .expect(1)
        .mount(&server)
        .await;

    let ctx = peer.context_builder().send().await.unwrap();
    assert_eq!(ctx.peer_id, "alice");
    assert_eq!(ctx.representation.as_deref(), Some("self context"));

    // A positive `query_param` matcher can only assert a parameter is PRESENT,
    // never absent. To prove that unset builder options are omitted entirely,
    // inspect the recorded request: with no option set, the context GET must
    // carry an empty query string (no `target`, `search_top_k`, etc.).
    let requests = server.received_requests().await.unwrap();
    let ctx_req = requests
        .iter()
        .find(|r| r.url.path().ends_with("/context"))
        .expect("context GET was recorded");
    let present: Vec<String> = ctx_req
        .url
        .query_pairs()
        .map(|(k, _)| k.into_owned())
        .collect();
    assert!(
        present.is_empty(),
        "unset context options must produce an empty query, got {present:?}"
    );
}

#[tokio::test]
#[allow(deprecated)]
async fn peer_context_with_target_deprecated_sends_query() {
    let (server, peer) = setup().await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/peers/alice/context"))
        .and(query_param("target", "bob"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "peer_id": "alice",
            "target_id": "bob",
            "representation": null,
            "peer_card": null
        })))
        .expect(1)
        .mount(&server)
        .await;

    let ctx = peer.context_with_target("bob").await.unwrap();
    assert_eq!(ctx.peer_id, "alice");
    assert_eq!(ctx.target_id, "bob");
}

#[tokio::test]
#[allow(deprecated)]
async fn peer_context_with_options_deprecated_sends_query() {
    let (server, peer) = setup().await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/peers/alice/context"))
        .and(query_param("target", "bob"))
        .and(query_param("search_query", "preferences"))
        .and(query_param("search_top_k", "7"))
        .and(query_param("max_conclusions", "15"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "peer_id": "alice",
            "target_id": "bob",
            "representation": "curated",
            "peer_card": null
        })))
        .expect(1)
        .mount(&server)
        .await;

    let opts = honcho_ai::types::peer::PeerContextOptions::builder()
        .target("bob")
        .search_query("preferences")
        .search_top_k(7)
        .max_conclusions(15)
        .build();
    let ctx = peer.context_with_options(&opts).await.unwrap();
    assert_eq!(ctx.target_id, "bob");
    assert_eq!(ctx.representation.as_deref(), Some("curated"));
}

#[tokio::test]
async fn peer_context_malformed_200_is_decode_error() {
    let (server, peer) = setup().await;

    // A 200 whose body does not match `PeerContext` (missing required fields)
    // must surface as a `Decode` error, never as a silently-defaulted value.
    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/peers/alice/context"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "unexpected": "shape"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let err = peer.context().await.unwrap_err();
    assert!(
        matches!(err, HonchoError::Decode { .. }),
        "malformed 200 body must surface as Decode, got {err:?}"
    );
    assert_eq!(err.status_code(), None);
}

// ── F5.7 Sessions ──────────────────────────────────────────────────

#[tokio::test]
async fn peer_sessions_returns_paginated() {
    let (server, peer) = setup().await;

    let active = common::session_response("s1");
    let inactive = json!({
        "id": "s2",
        "is_active": false,
        "workspace_id": "ws1",
        "metadata": {},
        "configuration": {},
        "created_at": "2025-01-16T10:30:00Z"
    });
    let body = common::page_json(vec![active, inactive], 2, 1, 50, 1);

    // `sessions()` sends no request body (filters are absent), so the contract
    // worth pinning is the default pagination query: page=1, size=50.
    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/alice/sessions"))
        .and(query_param("page", "1"))
        .and(query_param("size", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(&server)
        .await;

    let page = peer.sessions().await.unwrap();
    assert_eq!(page.total(), 2);
    assert_eq!(page.pages(), 1);
    assert_eq!(page.page(), 1);
    assert_eq!(page.size(), 50);
    let items = page.items();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].id, "s1");
    assert!(items[0].is_active);
    assert_eq!(items[1].id, "s2");
    assert!(!items[1].is_active);
}

#[tokio::test]
async fn peer_sessions_with_options_sends_filters_and_pagination() {
    use honcho_ai::types::session::SessionListOptions;

    let (server, peer) = setup().await;

    let mut filters = std::collections::HashMap::new();
    filters.insert("is_active".to_string(), json!(true));

    let body = common::page_json(vec![common::session_response("s1")], 1, 2, 10, 1);

    // With filters set, the POST body must carry `{"filters": …}` and the query
    // must reflect the requested page/size.
    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/alice/sessions"))
        .and(query_param("page", "2"))
        .and(query_param("size", "10"))
        .and(body_json(json!({ "filters": { "is_active": true } })))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(&server)
        .await;

    let opts = SessionListOptions::builder()
        .filters(filters)
        .page(2)
        .size(10)
        .build();
    let page = peer.sessions_with_options(&opts).await.unwrap();
    assert_eq!(page.page(), 2);
    assert_eq!(page.size(), 10);
    assert_eq!(page.total(), 1);
    assert_eq!(page.items()[0].id, "s1");
}

// ── F8.4 Streaming Chat ────────────────────────────────────────────

/// Wrap a single already-encoded JSON event payload as an SSE `data:` line.
fn sse_chunk(json: &str) -> String {
    format!("data: {json}\n\n")
}

/// Build a full SSE response body from a sequence of JSON event payloads.
///
/// Single canonical builder so every streaming test composes its body the same
/// way (one `data:` frame per event, including the terminal `{"done":true}`).
fn sse_body(events: &[&str]) -> String {
    events.iter().map(|e| sse_chunk(e)).collect()
}

#[tokio::test]
async fn chat_stream_basic() {
    let (server, peer) = setup().await;

    let body = sse_body(&[
        r#"{"delta":{"content":"hello"}}"#,
        r#"{"delta":{"content":" world"}}"#,
        r#"{"done":true}"#,
    ]);

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/alice/chat"))
        .and(body_json(json!({ "query": "hi", "stream": true })))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(body)
                .insert_header("content-type", "text/event-stream"),
        )
        .expect(1)
        .mount(&server)
        .await;

    let mut stream = peer.chat_stream("hi").send().await.unwrap();

    let mut chunks = Vec::new();
    while let Some(item) = stream.next().await {
        chunks.push(item.unwrap());
    }
    assert_eq!(chunks, vec!["hello", " world"]);
    // The adapter accumulates every yielded chunk into the final response.
    assert_eq!(stream.final_response().content(), "hello world");
}

#[tokio::test]
async fn chat_stream_with_target_session_reasoning_level() {
    let (server, peer) = setup().await;

    let body = sse_body(&[r#"{"delta":{"content":"response"}}"#, r#"{"done":true}"#]);

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/alice/chat"))
        .and(body_json(json!({
            "query": "deep thought",
            "stream": true,
            "target": "bob",
            "session_id": "sess42",
            "reasoning_level": "high"
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(body)
                .insert_header("content-type", "text/event-stream"),
        )
        .expect(1)
        .mount(&server)
        .await;

    let mut stream = peer
        .chat_stream("deep thought")
        .target("bob")
        .session("sess42")
        .reasoning_level(honcho_ai::types::dialectic::ReasoningLevel::High)
        .send()
        .await
        .unwrap();

    let mut chunks = Vec::new();
    while let Some(item) = stream.next().await {
        chunks.push(item.unwrap());
    }
    assert_eq!(chunks, vec!["response"]);
    assert_eq!(stream.final_response().content(), "response");
}

#[tokio::test]
async fn chat_stream_error_before_first_byte_returns_err() {
    let (server, peer) = setup().await;

    // POST is non-idempotent, so a 5xx is never retried: exactly one attempt.
    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/alice/chat"))
        .respond_with(ResponseTemplate::new(500))
        .expect(1)
        .mount(&server)
        .await;

    let err = peer.chat_stream("hi").send().await.unwrap_err();
    assert!(
        matches!(err, HonchoError::Server { status: 500, .. }),
        "expected Server(500), got {err:?}"
    );
    assert_eq!(err.status_code(), Some(500));
}

#[tokio::test]
async fn chat_stream_validates_non_empty_query() {
    let (server, peer) = setup().await;

    let err = peer.chat_stream("").send().await.unwrap_err();
    assert!(
        matches!(err, HonchoError::Validation(ref msg) if msg.contains("query")),
        "expected Validation error for empty query, got {err:?}"
    );

    // Validation short-circuits before any network call: no chat POST recorded.
    let requests = server.received_requests().await.unwrap();
    assert!(
        !requests.iter().any(|r| r.url.path().ends_with("/chat")),
        "empty-query validation must short-circuit before any chat request"
    );
}
