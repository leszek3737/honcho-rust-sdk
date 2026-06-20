//! Integration tests for Session core: F6.1–F6.3.
//!
//! Network discipline (see [`setup`]):
//! * Sync accessors (`id`/`is_active`/`metadata`/`configuration`/`created_at`)
//!   read the cache and MUST NOT hit the network — proven with `.expect(0)`.
//! * `refresh`/`get_metadata`/`get_configuration` issue a **GET** and update the
//!   cache — proven with `.expect(1)` on the GET mock.
//! * Mutators (`add_peers`/`set_peers`/`remove_peers`/`set_peer_configuration`)
//!   MUST issue exactly one request — proven with `.expect(1)` so a silent
//!   no-op (`Ok(())`, zero requests) fails instead of passing.

#![allow(
    // The SDK denies these crate-wide via `[lints.clippy]`; tests trade that
    // discipline for terse assertions and `panic!`-in-match-arm coverage of the
    // `#[non_exhaustive]` `PeerSpec` enum.
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::needless_pass_by_value,
    clippy::needless_borrows_for_generic_args,
    missing_docs
)]

mod common;

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use honcho_ai::Session;
use honcho_ai::error::HonchoError;
use honcho_ai::session::PeerSpec;
use honcho_ai::types::session::{SessionConfiguration, SessionPeerConfig};
use serde_json::{Value, json};
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use common::{make_honcho, mount_workspace_ensure, page_json, peer_response};

const SESSION_PATH: &str = "/v3/workspaces/ws1/sessions/sess1";
const PEERS_PATH: &str = "/v3/workspaces/ws1/sessions/sess1/peers";
/// Sessions collection — the get-or-create `POST` endpoint that session reads
/// use, since the server exposes no `GET /sessions/{id}` (only PUT/DELETE).
const SESSIONS_PATH: &str = "/v3/workspaces/ws1/sessions";

/// A `SessionResponse` wire body with caller-chosen metadata/configuration.
///
/// `common::session_response` returns empty maps; several tests need non-empty
/// metadata/config to exercise cache and full-replace semantics, so we build a
/// richer body here.
fn session_body(metadata: Value, configuration: Value) -> Value {
    json!({
        "id": "sess1",
        "workspace_id": "ws1",
        "is_active": true,
        "metadata": metadata,
        "configuration": configuration,
        "created_at": "2025-01-15T10:30:00Z",
    })
}

/// The session body returned by [`setup`] at construction time.
fn session_seed() -> Value {
    session_body(
        json!({"topic": "test"}),
        json!({"reasoning": {"enabled": true}}),
    )
}

fn peer_config(observe_me: Option<bool>, observe_others: Option<bool>) -> SessionPeerConfig {
    // `SessionPeerConfig` is `#[non_exhaustive]`, so it cannot be built with a
    // struct literal from this crate; deserialize instead.
    serde_json::from_value(json!({
        "observe_me": observe_me,
        "observe_others": observe_others,
    }))
    .unwrap()
}

/// Start the mock server's construction stubs and build a `Session`.
///
/// Mounts the workspace-ensure POST and the session get-or-create POST, then
/// drives `honcho.session("sess1").build()` (which calls both exactly once).
///
/// `up_to_n_times(1)` + fall-through: each construction stub answers its single
/// build-time call and is then **exhausted**, so wiremock falls through to any
/// later mock a test mounts on the same route instead of the leftover stub
/// shadowing it. `mount_workspace_ensure(.., 1)` additionally *verifies* exactly
/// one ensure call when the server drops.
async fn setup(server: &MockServer) -> Session {
    mount_workspace_ensure(server, 1).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/sessions"))
        .and(body_json(json!({"id": "sess1"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(session_seed()))
        .up_to_n_times(1)
        .mount(server)
        .await;

    make_honcho(&server.uri())
        .session("sess1")
        .build()
        .await
        .unwrap()
}

// ── F6.1: Construction + Metadata/Config CRUD ────────────────────────

/// Every cheap accessor in one build: `id`, `is_active`, cached metadata, cached
/// configuration, `created_at` — and the `.expect(0)` GET stub proves all of
/// them read the cache with **zero** network calls.
#[tokio::test]
async fn session_accessors_read_cache_without_network() {
    let server = MockServer::start().await;

    // Any GET against the session route here means an accessor leaked to the
    // network; `.expect(0)` fails the test on server drop if so.
    Mock::given(method("GET"))
        .and(path(SESSION_PATH))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;

    let session = setup(&server).await;

    assert_eq!(session.id(), "sess1");
    assert!(session.is_active());

    let meta = session.metadata().unwrap();
    assert_eq!(meta.get("topic").unwrap(), "test");

    let config = session.configuration().unwrap();
    assert_eq!(config.reasoning.unwrap().enabled, Some(true));

    let expected: DateTime<Utc> = "2025-01-15T10:30:00Z".parse().unwrap();
    assert_eq!(session.created_at(), expected);
}

#[tokio::test]
async fn session_refresh_updates_caches() {
    let server = MockServer::start().await;
    let session = setup(&server).await;

    let updated = session_body(
        json!({"topic": "updated", "priority": 1}),
        json!({"reasoning": {"enabled": false}}),
    );

    Mock::given(method("POST"))
        .and(path(SESSIONS_PATH))
        .and(body_json(json!({"id": "sess1"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(&updated))
        .expect(1)
        .mount(&server)
        .await;

    session.refresh().await.unwrap();

    let meta = session.metadata().unwrap();
    assert_eq!(meta.get("topic").unwrap(), "updated");
    assert_eq!(meta.get("priority").unwrap(), 1);

    let config = session.configuration().unwrap();
    assert_eq!(config.reasoning.unwrap().enabled, Some(false));
}

/// `get_metadata()` calls `refresh()` → get-or-create **POST**; it fetches fresh
/// state, it does not read the cache. The `.expect(1)` proves the network hit.
#[tokio::test]
async fn session_get_metadata_fetches_fresh() {
    let server = MockServer::start().await;
    let session = setup(&server).await;

    let updated = session_body(json!({"k": "v"}), json!({}));

    Mock::given(method("POST"))
        .and(path(SESSIONS_PATH))
        .and(body_json(json!({"id": "sess1"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(&updated))
        .expect(1)
        .mount(&server)
        .await;

    let meta = session.get_metadata().await.unwrap();
    assert_eq!(meta.get("k").unwrap(), "v");
}

/// `get_configuration()` calls `refresh()` → get-or-create **POST**; fetches
/// fresh, not cache.
#[tokio::test]
async fn session_get_configuration_fetches_fresh() {
    let server = MockServer::start().await;
    let session = setup(&server).await;

    let updated = session_body(json!({}), json!({"summary": {"enabled": true}}));

    Mock::given(method("POST"))
        .and(path(SESSIONS_PATH))
        .and(body_json(json!({"id": "sess1"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(&updated))
        .expect(1)
        .mount(&server)
        .await;

    let config = session.get_configuration().await.unwrap();
    assert_eq!(config.summary.unwrap().enabled, Some(true));
}

/// `set_metadata` is a **full PUT replace**: the seed `topic` is gone afterwards,
/// only the new keys survive in the cache.
#[tokio::test]
async fn session_set_metadata_replaces_via_put() {
    let server = MockServer::start().await;
    let session = setup(&server).await;

    let mut new_meta = HashMap::new();
    new_meta.insert("updated".to_owned(), json!(true));

    Mock::given(method("PUT"))
        .and(path(SESSION_PATH))
        .and(body_json(json!({"metadata": {"updated": true}})))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(session_body(json!({"updated": true}), json!({}))),
        )
        .expect(1)
        .mount(&server)
        .await;

    session.set_metadata(new_meta).await.unwrap();

    let cached = session.metadata().unwrap();
    assert_eq!(cached.get("updated").unwrap(), true);
    // Full replace: the construction-time `topic` key must be gone.
    assert!(!cached.contains_key("topic"));
}

#[tokio::test]
async fn session_set_configuration_puts_to_session_endpoint() {
    let server = MockServer::start().await;
    let session = setup(&server).await;

    let new_config: SessionConfiguration =
        serde_json::from_value(json!({"summary": {"enabled": false}})).unwrap();

    let resp = session_body(
        json!({"topic": "test"}),
        json!({"summary": {"enabled": false}}),
    );

    Mock::given(method("PUT"))
        .and(path(SESSION_PATH))
        .and(body_json(
            json!({"configuration": {"summary": {"enabled": false}}}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp))
        .expect(1)
        .mount(&server)
        .await;

    session.set_configuration(&new_config).await.unwrap();

    let cached = session.configuration().unwrap();
    assert_eq!(cached.summary.unwrap().enabled, Some(false));
}

// ── F6.1: Error paths (real `HonchoError` variants) ──────────────────

/// A get-or-create `POST` 404 surfaces as `NotFound`. 404 is non-retryable, so
/// the request fires exactly once.
#[tokio::test]
async fn session_refresh_propagates_not_found() {
    let server = MockServer::start().await;
    let session = setup(&server).await;

    Mock::given(method("POST"))
        .and(path(SESSIONS_PATH))
        .and(body_json(json!({"id": "sess1"})))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(json!({"detail": "session not found"})),
        )
        .expect(1)
        .mount(&server)
        .await;

    let err = session.refresh().await.unwrap_err();
    assert_eq!(err.status_code(), Some(404));
    assert!(matches!(err, HonchoError::NotFound { .. }));
}

/// A `POST` 500 surfaces as `Server { status: 500 }`. POST is non-idempotent, so
/// it is **not** retried — the request fires exactly once.
#[tokio::test]
async fn session_add_peers_propagates_server_error() {
    let server = MockServer::start().await;
    let session = setup(&server).await;

    Mock::given(method("POST"))
        .and(path(PEERS_PATH))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({"detail": "boom"})))
        .expect(1)
        .mount(&server)
        .await;

    let err = session.add_peers(["alice"]).await.unwrap_err();
    assert_eq!(err.status_code(), Some(500));
    assert!(matches!(err, HonchoError::Server { status: 500, .. }));
}

// ── F6.2: Peer Management ────────────────────────────────────────────

#[tokio::test]
async fn session_add_peer_posts_to_session_peers() {
    let server = MockServer::start().await;
    let session = setup(&server).await;

    Mock::given(method("POST"))
        .and(path(PEERS_PATH))
        .and(body_json(json!({"alice": {}})))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    session.add_peer("alice").await.unwrap();
}

#[tokio::test]
async fn session_add_peers_with_config() {
    let server = MockServer::start().await;
    let session = setup(&server).await;

    let cfg = peer_config(Some(true), Some(false));

    Mock::given(method("POST"))
        .and(path(PEERS_PATH))
        .and(body_json(json!({
            "alice": {"observe_me": true, "observe_others": false}
        })))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    session
        .add_peers([PeerSpec::WithConfig("alice".to_owned(), cfg)])
        .await
        .unwrap();
}

#[tokio::test]
async fn session_set_peers_puts_to_session_peers() {
    let server = MockServer::start().await;
    let session = setup(&server).await;

    Mock::given(method("PUT"))
        .and(path(PEERS_PATH))
        .and(body_json(json!({"bob": {}, "carol": {}})))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    session.set_peers(["bob", "carol"]).await.unwrap();
}

#[tokio::test]
async fn session_remove_peers_deletes_with_json_array_body() {
    let server = MockServer::start().await;
    let session = setup(&server).await;

    Mock::given(method("DELETE"))
        .and(path(PEERS_PATH))
        .and(body_json(json!(["alice", "bob"])))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    session.remove_peers(["alice", "bob"]).await.unwrap();
}

// Empty iterators still issue a request (no client-side short-circuit): the body
// is an empty object/array. These lock that behavior in.

#[tokio::test]
async fn session_add_peers_empty_posts_empty_object() {
    let server = MockServer::start().await;
    let session = setup(&server).await;

    Mock::given(method("POST"))
        .and(path(PEERS_PATH))
        .and(body_json(json!({})))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let empty: [&str; 0] = [];
    session.add_peers(empty).await.unwrap();
}

#[tokio::test]
async fn session_set_peers_empty_puts_empty_object() {
    let server = MockServer::start().await;
    let session = setup(&server).await;

    Mock::given(method("PUT"))
        .and(path(PEERS_PATH))
        .and(body_json(json!({})))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let empty: [&str; 0] = [];
    session.set_peers(empty).await.unwrap();
}

#[tokio::test]
async fn session_remove_peers_empty_deletes_empty_array() {
    let server = MockServer::start().await;
    let session = setup(&server).await;

    Mock::given(method("DELETE"))
        .and(path(PEERS_PATH))
        .and(body_json(json!([])))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let empty: [&str; 0] = [];
    session.remove_peers(empty).await.unwrap();
}

#[tokio::test]
async fn session_peers_flattens_single_page() {
    let server = MockServer::start().await;
    let session = setup(&server).await;

    Mock::given(method("GET"))
        .and(path(PEERS_PATH))
        .and(query_param("page", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page_json(
            vec![peer_response("alice"), peer_response("bob")],
            2,
            1,
            50,
            1,
        )))
        .expect(1)
        .mount(&server)
        .await;

    let peers = session.peers().await.unwrap();
    assert_eq!(peers.len(), 2);
    assert_eq!(peers[0].id(), "alice");
    assert_eq!(peers[1].id(), "bob");
}

/// `peers()` walks **every** page. Page 2's peer must not be dropped.
#[tokio::test]
async fn session_peers_walks_all_pages() {
    let server = MockServer::start().await;
    let session = setup(&server).await;

    Mock::given(method("GET"))
        .and(path(PEERS_PATH))
        .and(query_param("page", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page_json(
            vec![peer_response("alice")],
            2,
            1,
            50,
            2,
        )))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path(PEERS_PATH))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page_json(
            vec![peer_response("bob")],
            2,
            2,
            50,
            2,
        )))
        .expect(1)
        .mount(&server)
        .await;

    let peers = session.peers().await.unwrap();
    assert_eq!(peers.len(), 2);
    assert_eq!(peers[0].id(), "alice");
    assert_eq!(peers[1].id(), "bob");
}

#[tokio::test]
async fn session_peers_empty_returns_empty_vec() {
    let server = MockServer::start().await;
    let session = setup(&server).await;

    Mock::given(method("GET"))
        .and(path(PEERS_PATH))
        .and(query_param("page", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page_json(Vec::new(), 0, 1, 50, 1)))
        .expect(1)
        .mount(&server)
        .await;

    let peers = session.peers().await.unwrap();
    assert!(peers.is_empty());
}

// ── PeerSpec `From` conversions (no server) ──────────────────────────

#[test]
fn peer_spec_from_str() {
    match PeerSpec::from("alice") {
        PeerSpec::Id(id) => assert_eq!(id, "alice"),
        other => panic!("expected PeerSpec::Id, got {other:?}"),
    }
}

#[test]
fn peer_spec_from_string() {
    match PeerSpec::from(String::from("alice")) {
        PeerSpec::Id(id) => assert_eq!(id, "alice"),
        other => panic!("expected PeerSpec::Id, got {other:?}"),
    }
}

#[test]
fn peer_spec_from_tuple_str_config() {
    let cfg = peer_config(Some(true), None);
    match PeerSpec::from(("alice", cfg)) {
        PeerSpec::WithConfig(id, c) => {
            assert_eq!(id, "alice");
            assert_eq!(c.observe_me, Some(true));
        }
        other => panic!("expected PeerSpec::WithConfig, got {other:?}"),
    }
}

/// Previously untested `From<(String, SessionPeerConfig)>`.
#[test]
fn peer_spec_from_tuple_string_config() {
    let cfg = peer_config(Some(false), Some(true));
    match PeerSpec::from((String::from("alice"), cfg)) {
        PeerSpec::WithConfig(id, c) => {
            assert_eq!(id, "alice");
            assert_eq!(c.observe_others, Some(true));
        }
        other => panic!("expected PeerSpec::WithConfig, got {other:?}"),
    }
}

/// Previously untested `From<&Peer>`. A `Peer` has no public constructor, so it
/// is built through the client; the assertion itself is a pure conversion check.
#[tokio::test]
async fn peer_spec_from_peer_ref() {
    let server = MockServer::start().await;
    mount_workspace_ensure(&server, 1).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers"))
        .and(body_json(json!({"id": "alice"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(peer_response("alice")))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    let peer = make_honcho(&server.uri())
        .peer("alice")
        .build()
        .await
        .unwrap();

    match PeerSpec::from(&peer) {
        PeerSpec::Id(id) => assert_eq!(id, "alice"),
        other => panic!("expected PeerSpec::Id, got {other:?}"),
    }
}

// ── F6.3: Per-peer configuration ─────────────────────────────────────

#[tokio::test]
async fn session_get_peer_configuration_gets_config() {
    let server = MockServer::start().await;
    let session = setup(&server).await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/peers/alice/config"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "observe_me": true,
            "observe_others": false
        })))
        .expect(1)
        .mount(&server)
        .await;

    let cfg = session.get_peer_configuration("alice").await.unwrap();
    assert_eq!(cfg.observe_me, Some(true));
    assert_eq!(cfg.observe_others, Some(false));
}

#[tokio::test]
async fn session_set_peer_configuration_puts_config() {
    let server = MockServer::start().await;
    let session = setup(&server).await;

    let cfg = peer_config(Some(true), Some(false));

    Mock::given(method("PUT"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/peers/alice/config"))
        .and(body_json(json!({
            "observe_me": true,
            "observe_others": false
        })))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    session.set_peer_configuration("alice", &cfg).await.unwrap();
}

/// `set_peer_configuration` documents 404/`NotFound` for an absent peer (it does
/// not create peers). A `PUT` 404 is non-retryable, so it fires exactly once.
#[tokio::test]
async fn session_set_peer_configuration_absent_peer_is_not_found() {
    let server = MockServer::start().await;
    let session = setup(&server).await;

    let cfg = peer_config(Some(true), Some(false));

    Mock::given(method("PUT"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/peers/ghost/config"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"detail": "peer not found"})))
        .expect(1)
        .mount(&server)
        .await;

    let err = session
        .set_peer_configuration("ghost", &cfg)
        .await
        .unwrap_err();
    assert_eq!(err.status_code(), Some(404));
    assert!(matches!(err, HonchoError::NotFound { .. }));
}
