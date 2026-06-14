//! Tests for `Honcho::peer()` and `Honcho::session()` (F4.4).
//!
//! These exercise the get-or-create flow end-to-end against a `wiremock`
//! server: the lazy `ensure_workspace` POST, request-body serialization
//! (including `PeerSpec` -> peers map), empty-id validation, and an HTTP error
//! path. Shared fixtures (`make_honcho`, `peer_response`, `session_response`,
//! `mount_workspace_ensure`) live in `tests/common/mod.rs`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::needless_borrows_for_generic_args,
    clippy::unused_async,
    missing_docs
)]

mod common;

use std::collections::HashMap;

use honcho_ai::SessionPeerConfig;
use honcho_ai::error::HonchoError;
use honcho_ai::session::PeerSpec;
use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PEERS_PATH: &str = "/v3/workspaces/ws1/peers";
const SESSIONS_PATH: &str = "/v3/workspaces/ws1/sessions";
const ENSURE_PATH: &str = "/v3/workspaces";

/// Position (in arrival order) of the first recorded request hitting `path`.
async fn request_index(server: &MockServer, path: &str) -> Option<usize> {
    server
        .received_requests()
        .await
        .expect("wiremock records requests")
        .iter()
        .position(|r| r.url.path() == path)
}

// ── F4.4.1: peer makes get-or-create POST, returns Peer ───────────

#[tokio::test]
async fn peer_makes_get_or_create_post_returns_peer() {
    let server = MockServer::start().await;
    let honcho = common::make_honcho(&server.uri());

    common::mount_workspace_ensure(&server, 1).await;

    // Response carries metadata so we can assert it lands in the cache.
    let mut response = common::peer_response("alice");
    response["metadata"] = json!({ "role": "admin" });

    Mock::given(method("POST"))
        .and(path(PEERS_PATH))
        .and(body_json(json!({ "id": "alice" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .expect(1)
        .mount(&server)
        .await;

    let peer = honcho.peer("alice").build().await.unwrap();
    assert_eq!(peer.id(), "alice");

    // `metadata()` returns an owned map: use `get` so a missing key is `None`
    // rather than an indexing panic.
    let meta = peer.metadata().unwrap();
    assert_eq!(meta.get("role"), Some(&json!("admin")));
}

// ── F4.4.3: peer calls ensure_workspace first ──────────────────────

#[tokio::test]
async fn peer_calls_ensure_workspace_first() {
    let server = MockServer::start().await;
    let honcho = common::make_honcho(&server.uri());

    // `.expect(1)` proves the ensure POST is issued exactly once.
    common::mount_workspace_ensure(&server, 1).await;

    Mock::given(method("POST"))
        .and(path(PEERS_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(common::peer_response("alice")))
        .expect(1)
        .mount(&server)
        .await;

    let peer = honcho.peer("alice").build().await.unwrap();
    assert_eq!(peer.id(), "alice");

    // Ordering: the workspace-ensure POST must precede the peer create POST.
    let ensure_pos = request_index(&server, ENSURE_PATH)
        .await
        .expect("ensure POST recorded");
    let peer_pos = request_index(&server, PEERS_PATH)
        .await
        .expect("peer POST recorded");
    assert!(
        ensure_pos < peer_pos,
        "ensure_workspace POST (idx {ensure_pos}) must precede peer create POST (idx {peer_pos})"
    );
}

// ── ensure_workspace memoization: one ensure across many calls ──────

#[tokio::test]
async fn ensure_workspace_memoized_across_calls() {
    let server = MockServer::start().await;
    let honcho = common::make_honcho(&server.uri());

    // Memoized: a second peer() must NOT re-issue the ensure POST.
    common::mount_workspace_ensure(&server, 1).await;

    // Two distinct peer creates => two POSTs to the peers endpoint.
    Mock::given(method("POST"))
        .and(path(PEERS_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(common::peer_response("p")))
        .expect(2)
        .mount(&server)
        .await;

    honcho.peer("alice").build().await.unwrap();
    honcho.peer("bob").build().await.unwrap();
    // Exact counts are verified by wiremock when `server` drops.
}

// ── F4.4.4: peer create serializes metadata + configuration ─────────

#[tokio::test]
async fn peer_create_serializes_metadata_and_configuration() {
    let server = MockServer::start().await;
    let honcho = common::make_honcho(&server.uri());

    common::mount_workspace_ensure(&server, 1).await;

    let mut metadata = HashMap::new();
    metadata.insert("team".to_string(), json!("eng"));
    let mut configuration = HashMap::new();
    configuration.insert("observe_me".to_string(), json!(true));

    // Assert the exact wire body the builder produces for the optional fields.
    Mock::given(method("POST"))
        .and(path(PEERS_PATH))
        .and(body_json(json!({
            "id": "alice",
            "metadata": { "team": "eng" },
            "configuration": { "observe_me": true }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(common::peer_response("alice")))
        .expect(1)
        .mount(&server)
        .await;

    let peer = honcho
        .peer("alice")
        .metadata(metadata)
        .config(configuration)
        .build()
        .await
        .unwrap();
    assert_eq!(peer.id(), "alice");
}

// ── F4.4.5: session makes get-or-create POST ───────────────────────

#[tokio::test]
async fn session_makes_get_or_create_post() {
    let server = MockServer::start().await;
    let honcho = common::make_honcho(&server.uri());

    common::mount_workspace_ensure(&server, 1).await;

    let mut response = common::session_response("sess1");
    response["metadata"] = json!({ "env": "test" });

    Mock::given(method("POST"))
        .and(path(SESSIONS_PATH))
        .and(body_json(json!({ "id": "sess1" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .expect(1)
        .mount(&server)
        .await;

    let session = honcho.session("sess1").build().await.unwrap();
    assert_eq!(session.id(), "sess1");
    assert!(session.is_active());

    let meta = session.metadata().unwrap();
    assert_eq!(meta.get("env"), Some(&json!("test")));
}

// ── F4.4.6: session serializes PeerSpec list into a peers map ───────

#[tokio::test]
async fn session_serializes_peers_into_map() {
    let server = MockServer::start().await;
    let honcho = common::make_honcho(&server.uri());

    common::mount_workspace_ensure(&server, 1).await;

    // `#[non_exhaustive]` forbids a struct literal here, so build from
    // `default()` and set fields.
    let mut carol_cfg = SessionPeerConfig::default();
    carol_cfg.observe_me = Some(true);
    carol_cfg.observe_others = Some(false);

    let peers = vec![
        PeerSpec::Id("bob".to_owned()),
        PeerSpec::WithConfig("carol".to_owned(), carol_cfg),
    ];

    let mut metadata = HashMap::new();
    metadata.insert("topic".to_string(), json!("rust"));

    // `PeerSpec::Id` collapses to a default (empty) per-peer config; the
    // `WithConfig` variant carries its observation flags. Both land in the
    // `peers` object keyed by peer id.
    Mock::given(method("POST"))
        .and(path(SESSIONS_PATH))
        .and(body_json(json!({
            "id": "sess1",
            "metadata": { "topic": "rust" },
            "peers": {
                "bob": {},
                "carol": { "observe_me": true, "observe_others": false }
            }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(common::session_response("sess1")))
        .expect(1)
        .mount(&server)
        .await;

    let session = honcho
        .session("sess1")
        .metadata(metadata)
        .peers(peers)
        .build()
        .await
        .unwrap();
    assert_eq!(session.id(), "sess1");
}

// ── Validation: empty id short-circuits before any HTTP ─────────────

#[tokio::test]
async fn peer_empty_id_is_configuration_error() {
    let server = MockServer::start().await;
    let honcho = common::make_honcho(&server.uri());

    let err = honcho.peer("").build().await.unwrap_err();
    assert!(
        matches!(err, HonchoError::Configuration(_)),
        "empty peer id must be a Configuration error, got {err:?}"
    );
    assert_eq!(err.code(), "configuration_error");

    // Validation must run before `ensure_workspace`: no request reaches the
    // server.
    assert!(
        server
            .received_requests()
            .await
            .expect("wiremock records requests")
            .is_empty(),
        "empty-id validation must short-circuit before any HTTP request"
    );
}

#[tokio::test]
async fn session_empty_id_is_configuration_error() {
    let server = MockServer::start().await;
    let honcho = common::make_honcho(&server.uri());

    // `Session` is not `Debug`, so go via `.err()` rather than `unwrap_err()`.
    let err = honcho
        .session("")
        .build()
        .await
        .err()
        .expect("empty session id must error");
    assert!(
        matches!(err, HonchoError::Configuration(_)),
        "empty session id must be a Configuration error, got {err:?}"
    );
    assert_eq!(err.code(), "configuration_error");

    assert!(
        server
            .received_requests()
            .await
            .expect("wiremock records requests")
            .is_empty(),
        "empty-id validation must short-circuit before any HTTP request"
    );
}

// ── HTTP error path: 422 from the peer create surfaces typed error ──

#[tokio::test]
async fn peer_create_http_error_surfaces() {
    let server = MockServer::start().await;
    let honcho = common::make_honcho(&server.uri());

    common::mount_workspace_ensure(&server, 1).await;

    // 422 is not retryable, so the peer POST is hit exactly once.
    Mock::given(method("POST"))
        .and(path(PEERS_PATH))
        .respond_with(
            ResponseTemplate::new(422).set_body_json(json!({ "detail": "invalid peer" })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let err = honcho.peer("alice").build().await.unwrap_err();
    assert_eq!(err.status_code(), Some(422));
    assert!(
        matches!(err, HonchoError::UnprocessableEntity { .. }),
        "expected UnprocessableEntity, got {err:?}"
    );
}
