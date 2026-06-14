//! Wire-format regression tests for session peer management.
//!
//! Verifies that `add_peers`, `set_peers`, `remove_peers`, and `peers` send and
//! receive the correct JSON payloads: a flat `{id: config}` map for POST/PUT (no
//! `{"peers": …}` wrapper), a bare `[id, …]` list for DELETE, and a paginated
//! page for the GET. The config path also locks the `skip_serializing_if` on
//! `SessionPeerConfig::observe_me` / `observe_others`.
//!
//! Shared fixtures (`make_honcho`, `session_response`, `mount_workspace_ensure`,
//! `peer_response`, `page_json`) live in `tests/common/mod.rs` and are reused
//! here rather than re-declared.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

mod common;

use std::future::Future;

use honcho_ai::error::HonchoError;
use honcho_ai::session::Session;
use honcho_ai::types::session::SessionPeerConfig;
use serde_json::{Value, json};
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Path of the session-peers collection endpoint shared by every verb.
const PEERS_PATH: &str = "/v3/workspaces/ws1/sessions/sess1/peers";

/// Builds a session against `server` by mounting the workspace-ensure POST and
/// the session create POST, then driving `Honcho::session(..).build()`.
async fn make_session(server: &MockServer) -> Session {
    common::mount_workspace_ensure(server, 1).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/sessions"))
        .and(body_json(json!({ "id": "sess1" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(common::session_response("sess1")))
        .expect(1)
        .mount(server)
        .await;

    common::make_honcho(&server.uri())
        .session("sess1")
        .build()
        .await
        .unwrap()
}

/// The flat two-peer payload shared by the `add_peers` / `set_peers` twins.
///
/// Owned (`Value`, not `&Value`) so call sites hand it straight to `body_json`
/// without a borrow, which is why this file needs no
/// `clippy::needless_borrows_for_generic_args` allow.
fn flat_two_peers() -> Value {
    json!({ "alice": {}, "bob": {} })
}

/// Mounts a single-call expectation on `PEERS_PATH` for `http_method` with the
/// exact JSON `expected` body plus a JSON content-type, runs `call`, and lets
/// wiremock verify the call count on server drop.
///
/// This collapses the otherwise-identical `add_peers` (POST), `set_peers` (PUT),
/// and `remove_peers` (DELETE) wire-format assertions into one place; `expected`
/// is a flat map for POST/PUT and an id list for DELETE.
async fn assert_flat_map<F, Fut>(http_method: &str, expected: Value, call: F)
where
    F: FnOnce(Session) -> Fut,
    Fut: Future<Output = honcho_ai::error::Result<()>>,
{
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    Mock::given(method(http_method))
        .and(path(PEERS_PATH))
        .and(header("content-type", "application/json"))
        .and(body_json(expected))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    call(session).await.unwrap();
}

// ── add_peers ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn add_peers_sends_flat_map_without_peers_wrapper() {
    assert_flat_map("POST", flat_two_peers(), |s| async move {
        s.add_peers(["alice", "bob"]).await
    })
    .await;
}

#[tokio::test]
async fn add_peer_single_sends_flat_map() {
    assert_flat_map("POST", json!({ "alice": {} }), |s| async move {
        s.add_peer("alice").await
    })
    .await;
}

/// CRITICAL coverage: the `PeerSpec::WithConfig` path through `normalize_peers`.
///
/// `observe_me: Some(true)` must serialize to `{"alice": {"observe_me": true}}`
/// — `observe_others` stays absent, proving the `skip_serializing_if` on the
/// unset field actually fires (acceptance #7). `SessionPeerConfig` is
/// `#[non_exhaustive]`, so it is built via `default` + field assignment.
#[tokio::test]
async fn add_peers_with_config_serializes_only_set_fields() {
    let mut cfg = SessionPeerConfig::default();
    cfg.observe_me = Some(true);

    assert_flat_map(
        "POST",
        json!({ "alice": { "observe_me": true } }),
        move |s| async move { s.add_peers([("alice", cfg)]).await },
    )
    .await;
}

#[tokio::test]
async fn add_peers_empty_sends_empty_object() {
    assert_flat_map("POST", json!({}), |s| async move {
        s.add_peers(Vec::<&str>::new()).await
    })
    .await;
}

/// Duplicate ids are rejected up front with a `Validation` error: the SDK does
/// NOT silently last-wins / drop an entry, so no request is ever sent.
#[tokio::test]
async fn add_peers_duplicate_ids_returns_validation_error() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    // No peers mock is mounted: the call must fail before any HTTP happens.
    let err = session.add_peers(["alice", "alice"]).await.unwrap_err();
    assert!(
        matches!(err, HonchoError::Validation(_)),
        "expected Validation for duplicate ids, got: {err:?}"
    );
}

// ── set_peers ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn set_peers_sends_flat_map_without_peers_wrapper() {
    assert_flat_map("PUT", flat_two_peers(), |s| async move {
        s.set_peers(["alice", "bob"]).await
    })
    .await;
}

#[tokio::test]
async fn set_peer_single_sends_flat_map() {
    assert_flat_map("PUT", json!({ "alice": {} }), |s| async move {
        s.set_peers(["alice"]).await
    })
    .await;
}

#[tokio::test]
async fn set_peers_with_config_serializes_only_set_fields() {
    let mut cfg = SessionPeerConfig::default();
    cfg.observe_others = Some(false);

    assert_flat_map(
        "PUT",
        json!({ "alice": { "observe_others": false } }),
        move |s| async move { s.set_peers([("alice", cfg)]).await },
    )
    .await;
}

// ── remove_peers ───────────────────────────────────────────────────────────

#[tokio::test]
async fn remove_peers_sends_list_of_ids() {
    assert_flat_map("DELETE", json!(["alice", "bob"]), |s| async move {
        s.remove_peers(["alice", "bob"]).await
    })
    .await;
}

#[tokio::test]
async fn remove_peer_single_sends_singleton_list() {
    assert_flat_map("DELETE", json!(["bob"]), |s| async move {
        s.remove_peers(["bob"]).await
    })
    .await;
}

#[tokio::test]
async fn remove_peers_empty_sends_empty_list() {
    assert_flat_map("DELETE", json!([]), |s| async move {
        s.remove_peers(Vec::<&str>::new()).await
    })
    .await;
}

// ── peers (GET) ──────────────────────────────────────────────────────────────

/// `peers()` GETs the paginated collection and deserializes each item into a
/// `Peer`; a single-page response yields the expected peer id.
#[tokio::test]
async fn peers_get_deserializes_page() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    Mock::given(method("GET"))
        .and(path(PEERS_PATH))
        // The SDK paginates with a bare `?page=1` (no `size`), so tighten the
        // matcher to that single query param.
        .and(query_param("page", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(common::page_json(
            vec![common::peer_response("alice")],
            1,
            1,
            50,
            1,
        )))
        .expect(1)
        .mount(&server)
        .await;

    let peers = session.peers().await.unwrap();
    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0].id(), "alice");
}
