//! Integration tests for Peer core, chat, search, and card methods (Phase 5).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::needless_pass_by_value,
    clippy::needless_borrows_for_generic_args,
    clippy::unused_async,
    clippy::items_after_statements,
    missing_docs
)]

use std::collections::HashMap;

use honcho_ai::Peer;
use honcho_ai::PeerConfig;
use honcho_ai::error::HonchoError;
use serde_json::{Value, json};
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

mod common;
use common::{make_honcho, mount_workspace_ensure};

/// Number of HTTP requests [`setup`] issues before a test runs: the
/// workspace-ensure `POST /v3/workspaces` and the peer get-or-create
/// `POST /v3/workspaces/ws1/peers`. Tests assert against this to prove a method
/// added (or skipped) network calls beyond peer construction, instead of
/// hard-coding the magic literal `2` at every call site.
const SETUP_REQUESTS: usize = 2;

fn peer_response_json() -> Value {
    peer_response_with(json!({"role": "admin"}), json!({"observe_me": true}))
}

fn peer_response_with(metadata: Value, configuration: Value) -> Value {
    json!({
        "id": "alice",
        "workspace_id": "ws1",
        "created_at": "2025-01-15T10:30:00Z",
        "metadata": metadata,
        "configuration": configuration
    })
}

/// Starts a mock server, mounts the workspace-ensure + peer get-or-create
/// handshake, and returns the server together with a freshly built `alice`
/// peer whose cache holds `{"role": "admin"}` / `{"observe_me": true}`.
///
/// Replaces the ~29 copy-pasted `MockServer::start` + `make_peer` blocks. The
/// peer get-or-create mock is `up_to_n_times(1)`, so tests that exercise a
/// later `POST /v3/workspaces/ws1/peers` (refresh / fetch) mount their own
/// follow-up mock without colliding with construction.
async fn setup() -> (MockServer, Peer) {
    let server = MockServer::start().await;
    mount_workspace_ensure(&server, 1).await;
    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers"))
        .and(body_json(&json!({"id": "alice"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(peer_response_json()))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    let peer = make_honcho(&server.uri())
        .peer("alice")
        .build()
        .await
        .unwrap();
    (server, peer)
}

/// Total number of requests the mock server has received so far.
async fn request_count(server: &MockServer) -> usize {
    server.received_requests().await.unwrap().len()
}

/// Number of received requests whose URL path ends with `suffix`.
///
/// Used instead of a magic total-count literal when a test only cares whether
/// a *specific* endpoint was hit (e.g. that a rejected request never reached
/// `/chat`), keeping the assertion decoupled from [`setup`]'s internals.
async fn count_path(server: &MockServer, suffix: &str) -> usize {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path().ends_with(suffix))
        .count()
}

// ── F5.1: Construction + Metadata ──────────────────────────────────────

#[tokio::test]
async fn peer_refresh_updates_caches() {
    let (server, peer) = setup().await;

    let updated = peer_response_with(
        json!({"role": "user", "level": 5}),
        json!({"observe_me": false, "observe_others": true}),
    );

    // `refresh()` issues a `POST` to the peers get-or-create endpoint (see
    // `Peer::fetch_and_update_cache`), *not* a `GET`.
    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&updated))
        .mount(&server)
        .await;

    peer.refresh().await.unwrap();

    let meta = peer.metadata().unwrap();
    assert_eq!(meta.get("role").unwrap(), "user");
    assert_eq!(meta.get("level").unwrap(), 5);

    let config = peer.configuration().unwrap();
    assert_eq!(config.observe_me, Some(false));
    assert_eq!(config.observe_others, Some(true));
}

#[tokio::test]
async fn peer_get_metadata_fetches_fresh() {
    // `get_metadata()` is NOT a cache read: it calls `fetch_and_update_cache`,
    // which `POST`s to the peers get-or-create endpoint and returns the
    // server's fresh metadata. The setup cache holds `{"role": "admin"}`; the
    // fetch returns `{"k": "v"}`, so observing `k` proves a network round-trip.
    let (server, peer) = setup().await;

    let updated = peer_response_with(json!({"k": "v"}), json!({}));
    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&updated))
        .mount(&server)
        .await;

    let meta = peer.get_metadata().await.unwrap();
    assert_eq!(meta.get("k").unwrap(), "v");
    // The freshly fetched value replaced the setup cache.
    assert!(!peer.metadata().unwrap().contains_key("role"));
    // Exactly one network call beyond construction: the fetch hit the wire.
    assert_eq!(request_count(&server).await, SETUP_REQUESTS + 1);
}

#[tokio::test]
async fn peer_metadata_accessor_uses_cache_no_network() {
    // The sync accessor `peer.metadata()` returns the cached snapshot with zero
    // network calls — the inverse of `get_metadata()` above.
    let (server, peer) = setup().await;

    let meta = peer.metadata().unwrap();
    assert_eq!(meta.get("role").unwrap(), "admin");

    // No request was issued beyond construction.
    assert_eq!(request_count(&server).await, SETUP_REQUESTS);
}

#[tokio::test]
async fn peer_set_metadata_puts_to_peer_endpoint() {
    let (server, peer) = setup().await;

    let mut new_meta = HashMap::new();
    new_meta.insert("updated".to_owned(), json!(true));

    let resp = peer_response_with(json!({"updated": true}), json!({"language": "en"}));

    Mock::given(method("PUT"))
        .and(path("/v3/workspaces/ws1/peers/alice"))
        .and(body_json(&json!({"metadata": {"updated": true}})))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp))
        .mount(&server)
        .await;

    peer.set_metadata(new_meta).await.unwrap();

    let cached = peer.metadata().unwrap();
    assert_eq!(cached.get("updated").unwrap(), true);
}

#[tokio::test]
async fn peer_set_configuration_puts_to_peer_endpoint() {
    let (server, peer) = setup().await;

    let new_config: PeerConfig = serde_json::from_value(json!({"observe_me": true})).unwrap();

    let resp = peer_response_with(json!({"role": "admin"}), json!({"observe_me": true}));

    Mock::given(method("PUT"))
        .and(path("/v3/workspaces/ws1/peers/alice"))
        .and(body_json(&json!({"configuration": {"observe_me": true}})))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp))
        .mount(&server)
        .await;

    peer.set_configuration(&new_config).await.unwrap();

    let cached = peer.configuration().unwrap();
    assert_eq!(cached.observe_me, Some(true));
}

#[tokio::test]
async fn peer_get_configuration_fetches_fresh() {
    // Like `get_metadata`, `get_configuration()` fetches over the network
    // (`POST` get-or-create) and returns the server's configuration, not the
    // cache. Setup cached `observe_me: true` only; the fetch adds
    // `observe_others: false`, proving the response — not the cache — was read.
    let (server, peer) = setup().await;

    let updated = peer_response_with(
        json!({}),
        json!({"observe_me": true, "observe_others": false}),
    );
    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&updated))
        .mount(&server)
        .await;

    let config = peer.get_configuration().await.unwrap();
    assert_eq!(config.observe_me, Some(true));
    assert_eq!(config.observe_others, Some(false));
    assert_eq!(request_count(&server).await, SETUP_REQUESTS + 1);
}

#[tokio::test]
async fn peer_configuration_accessor_uses_cache_no_network() {
    // Sync accessor returns the cached configuration with zero network calls.
    let (server, peer) = setup().await;

    let config = peer.configuration().unwrap();
    assert_eq!(config.observe_me, Some(true));
    assert_eq!(config.observe_others, None);

    assert_eq!(request_count(&server).await, SETUP_REQUESTS);
}

// ── F5.2: Chat (non-streaming) ────────────────────────────────────────

#[tokio::test]
async fn peer_chat_basic_query() {
    let (server, peer) = setup().await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/alice/chat"))
        .and(body_json(&json!({"query": "hello"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": "Hi there!"
        })))
        .mount(&server)
        .await;

    let result = peer.chat("hello").await.unwrap();
    assert_eq!(result, Some("Hi there!".to_owned()));
}

#[tokio::test]
async fn peer_chat_empty_content_returns_none() {
    let (server, peer) = setup().await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/alice/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": null
        })))
        .mount(&server)
        .await;

    let result = peer.chat("hello").await.unwrap();
    assert_eq!(result, None);
}

#[tokio::test]
async fn peer_chat_empty_string_content_returns_none() {
    let (server, peer) = setup().await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/alice/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": ""
        })))
        .mount(&server)
        .await;

    let result = peer.chat("hello").await.unwrap();
    assert_eq!(result, None);
}

#[tokio::test]
async fn peer_chat_validates_empty_query() {
    let (server, peer) = setup().await;

    let err = peer.chat("").await.unwrap_err();
    assert_eq!(err.code(), "validation_error");
    // Validation happens client-side: the request never reached `/chat`.
    assert_eq!(count_path(&server, "/chat").await, 0);
}

#[tokio::test]
async fn peer_chat_validates_whitespace_only_query() {
    let (server, peer) = setup().await;

    // A whitespace-only query is rejected like the empty string: the dialectic
    // validator trims before the emptiness check.
    let err = peer.chat("   ").await.unwrap_err();
    assert!(matches!(err, HonchoError::Validation(_)));
    assert_eq!(err.code(), "validation_error");
    assert_eq!(count_path(&server, "/chat").await, 0);
}

#[tokio::test]
async fn peer_chat_with_session_and_target() {
    let (server, peer) = setup().await;

    // Verify `session_id` and `target` actually reach the wire via `body_json`.
    // `stream: false` and the default reasoning level are skipped by serde, so
    // they must not appear in the request body.
    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/alice/chat"))
        .and(body_json(&json!({
            "query": "what do you know?",
            "session_id": "sess1",
            "target": "bob"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": "Bob likes Rust"
        })))
        .mount(&server)
        .await;

    use honcho_ai::types::dialectic::DialecticOptions;
    let options = DialecticOptions::builder()
        .query("what do you know?")
        .stream(false)
        .session_id("sess1")
        .target("bob")
        .build();

    let result = peer.chat_with_options(&options).await.unwrap();
    assert_eq!(result, Some("Bob likes Rust".to_owned()));
}

#[tokio::test]
async fn peer_chat_with_options_rejects_long_query_without_request() {
    let (server, peer) = setup().await;

    use honcho_ai::types::dialectic::DialecticOptions;
    let options = DialecticOptions::builder()
        .query("a".repeat(10_001))
        .stream(false)
        .build();

    let err = peer.chat_with_options(&options).await.unwrap_err();
    assert_eq!(err.code(), "validation_error");
    assert_eq!(err.message(), "query must be at most 10000 characters");

    // The oversized query is rejected before any chat request is sent. Filter
    // by path rather than asserting a magic total request count.
    assert_eq!(count_path(&server, "/chat").await, 0);
}

#[tokio::test]
async fn peer_chat_server_error_maps_to_server_variant() {
    let (server, peer) = setup().await;

    // `chat` is a `POST` (non-idempotent), so a 5xx is surfaced immediately
    // without retries — deterministic with the default retry policy.
    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/alice/chat"))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({"detail": "boom"})))
        .mount(&server)
        .await;

    let err = peer.chat("hello").await.unwrap_err();
    assert!(matches!(err, HonchoError::Server { status: 500, .. }));
    assert_eq!(err.status_code(), Some(500));
    // POST is not retried: exactly one chat attempt hit the wire.
    assert_eq!(count_path(&server, "/chat").await, 1);
}

// ── F5.3: Search ──────────────────────────────────────────────────────

#[tokio::test]
async fn peer_search_returns_messages() {
    let (server, peer) = setup().await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/alice/search"))
        .and(body_json(&json!({
            "query": "hello",
            "limit": 10
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "id": "msg1",
                "content": "hello world",
                "peer_id": "alice",
                "session_id": "sess1",
                "metadata": {},
                "created_at": "2025-01-15T10:30:00Z",
                "workspace_id": "ws1",
                "token_count": 2
            }
        ])))
        .mount(&server)
        .await;

    let results = peer.search("hello").await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id(), "msg1");
    assert_eq!(results[0].content(), "hello world");
    assert_eq!(results[0].peer_id(), "alice");
    assert_eq!(results[0].session_id(), "sess1");
    assert_eq!(results[0].token_count(), 2);
    let expected_created: chrono::DateTime<chrono::Utc> = "2025-01-15T10:30:00Z".parse().unwrap();
    assert_eq!(results[0].created_at(), expected_created);
}

#[tokio::test]
async fn peer_search_returns_empty_vec() {
    let (server, peer) = setup().await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/alice/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;

    let results = peer.search("test").await.unwrap();
    assert_eq!(results.len(), 0);
}

#[tokio::test]
async fn peer_search_with_options_sends_custom_limit() {
    let (server, peer) = setup().await;

    use honcho_ai::types::message::MessageSearchOptions;

    // Confirm a non-default `limit` reaches the wire via `body_json`.
    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/alice/search"))
        .and(body_json(&json!({
            "query": "topic",
            "limit": 20
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "id": "msg9",
                "content": "topic hit",
                "peer_id": "alice",
                "session_id": "sess1",
                "metadata": {},
                "created_at": "2025-01-15T10:30:00Z",
                "workspace_id": "ws1",
                "token_count": 4
            }
        ])))
        .mount(&server)
        .await;

    let options = MessageSearchOptions::builder().query("topic").limit(20).build();
    let results = peer.search_with_options(&options).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id(), "msg9");
}

#[tokio::test]
async fn peer_search_validates_empty_query() {
    let (server, peer) = setup().await;

    // An empty query is rejected client-side with a `Validation` error; the
    // request never reaches `/search`.
    let err = peer.search("").await.unwrap_err();
    assert!(matches!(err, HonchoError::Validation(_)));
    assert_eq!(err.code(), "validation_error");
    assert_eq!(count_path(&server, "/search").await, 0);
}

#[tokio::test]
async fn peer_search_unprocessable_maps_to_variant() {
    let (server, peer) = setup().await;

    // 422 is non-retryable, so a single `POST` settles the result.
    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/alice/search"))
        .respond_with(ResponseTemplate::new(422).set_body_json(json!({"detail": "bad query"})))
        .mount(&server)
        .await;

    let err = peer.search("hello").await.unwrap_err();
    assert!(matches!(err, HonchoError::UnprocessableEntity { .. }));
    assert_eq!(err.status_code(), Some(422));
}

// ── F5.4: Card ────────────────────────────────────────────────────────

#[tokio::test]
async fn peer_get_card_returns_vec() {
    let (server, peer) = setup().await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/peers/alice/card"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "peer_card": ["fact1", "fact2"]
        })))
        .mount(&server)
        .await;

    let card = peer.get_card().await.unwrap();
    assert_eq!(card, Some(vec!["fact1".to_owned(), "fact2".to_owned()]));
}

#[tokio::test]
async fn peer_get_card_with_target_sends_query() {
    let (server, peer) = setup().await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/peers/alice/card"))
        .and(query_param("target", "bob"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "peer_card": ["knows bob"]
        })))
        .mount(&server)
        .await;

    let card = peer.get_card_with_target("bob").await.unwrap();
    assert_eq!(card, Some(vec!["knows bob".to_owned()]));
}

#[tokio::test]
async fn peer_get_card_none_when_null() {
    let (server, peer) = setup().await;

    // `peer_card: null` deserializes to `None`.
    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/peers/alice/card"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "peer_card": null
        })))
        .mount(&server)
        .await;

    let card = peer.get_card().await.unwrap();
    assert_eq!(card, None);
}

#[tokio::test]
async fn peer_get_card_empty_array_is_some() {
    let (server, peer) = setup().await;

    // An empty array is distinct from `null`: `[]` deserializes to
    // `Some(vec![])`, not `None`.
    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/peers/alice/card"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "peer_card": []
        })))
        .mount(&server)
        .await;

    let card = peer.get_card().await.unwrap();
    assert_eq!(card, Some(vec![]));
}

#[tokio::test]
async fn peer_get_card_not_found_maps_to_variant() {
    let (server, peer) = setup().await;

    // 404 is non-retryable even though `GET` is idempotent, so the single
    // request settles deterministically.
    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/peers/alice/card"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"detail": "no card"})))
        .mount(&server)
        .await;

    let err = peer.get_card().await.unwrap_err();
    assert!(matches!(err, HonchoError::NotFound { .. }));
    assert_eq!(err.status_code(), Some(404));
}

#[tokio::test]
async fn peer_set_card_puts_card() {
    let (server, peer) = setup().await;

    Mock::given(method("PUT"))
        .and(path("/v3/workspaces/ws1/peers/alice/card"))
        .and(body_json(&json!({
            "peer_card": ["new fact"]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "peer_card": ["new fact"]
        })))
        .mount(&server)
        .await;

    let card = peer.set_card(vec!["new fact".to_owned()]).await.unwrap();
    assert_eq!(card, Some(vec!["new fact".to_owned()]));
}

#[tokio::test]
async fn peer_set_card_with_target_sends_query() {
    let (server, peer) = setup().await;

    Mock::given(method("PUT"))
        .and(path("/v3/workspaces/ws1/peers/alice/card"))
        .and(query_param("target", "bob"))
        .and(body_json(&json!({
            "peer_card": ["fact about bob"]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "peer_card": ["fact about bob"]
        })))
        .mount(&server)
        .await;

    let card = peer
        .set_card_with_target(vec!["fact about bob".to_owned()], "bob")
        .await
        .unwrap();
    assert_eq!(card, Some(vec!["fact about bob".to_owned()]));
}

// ── F5.8: Message builder tests ───────────────────────────────────────

#[tokio::test]
async fn peer_message_builder_does_not_call_api() {
    let (server, peer) = setup().await;

    let _msg = peer.message("hello").build().unwrap();

    // The builder is purely synchronous: no request beyond construction.
    assert_eq!(request_count(&server).await, SETUP_REQUESTS);
}

#[tokio::test]
async fn peer_message_builder_fields() {
    use honcho_ai::types::message::MessageConfiguration;

    let (_server, peer) = setup().await;

    let msg = peer
        .message("hello")
        .metadata(HashMap::from([("k".to_owned(), json!("v"))]))
        .configuration(MessageConfiguration::default())
        .build()
        .unwrap();

    assert_eq!(msg.peer_id, "alice");
    assert_eq!(msg.content, "hello");
    assert_eq!(msg.metadata.as_ref().unwrap().get("k").unwrap(), "v");
    assert_eq!(msg.configuration, Some(MessageConfiguration::default()));
}

#[tokio::test]
async fn peer_message_whitespace_only_is_rejected() {
    let (_server, peer) = setup().await;

    let err = peer.message("   ").build().unwrap_err();
    assert!(matches!(err, HonchoError::Validation(_)));
}

#[tokio::test]
async fn peer_message_empty_string_is_rejected() {
    let (_server, peer) = setup().await;

    let err = peer.message("").build().unwrap_err();
    assert!(matches!(err, HonchoError::Validation(_)));
}
