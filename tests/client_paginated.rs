#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::needless_pass_by_value,
    missing_docs
)]

//! Client-level pagination tests (`Honcho::peers` / `sessions` / `workspaces`
//! and their `*_with_filters` variants).
//!
//! Every mock carries an explicit `.expect(..)` so a double-fetch or a skipped
//! fetch is caught on server drop, and every request is pinned with
//! `query_param` / `body_json` matchers rather than echoing the response body.

mod common;

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use honcho_ai::error::HonchoError;
use honcho_ai::types::pagination::Page;
use honcho_ai::types::peer::Peer;
use honcho_ai::types::session::SessionResponse;
use honcho_ai::types::workspace::Workspace;
use serde_json::json;
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The fixed `created_at` every shared response builder emits, parsed into the
/// by-value `DateTime<Utc>` the typed models expose.
///
/// Comparing against this value (not a `String`) pins `created_at` to its
/// by-value `DateTime<Utc>` type: a regression to `String` or `&DateTime`
/// would fail to compile here.
fn expected_created_at() -> DateTime<Utc> {
    "2025-01-15T10:30:00Z"
        .parse()
        .expect("fixture timestamp is valid RFC 3339")
}

// ════════════════════════════════════════════════════════════════════════
// peers()
// ════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn peers_returns_paginated_typed() {
    let server = MockServer::start().await;
    let honcho = common::make_honcho(&server.uri());
    common::mount_workspace_ensure(&server, 1).await;

    let body = common::page_json(
        vec![common::peer_response("alice"), common::peer_response("bob")],
        2,
        1,
        50,
        1,
    );
    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/list"))
        .and(query_param("page", "1"))
        .and(query_param("size", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(&server)
        .await;

    let page: Page<Peer> = honcho.peers().await.unwrap();

    // Pagination envelope.
    assert_eq!(page.total(), 2);
    assert_eq!(page.page(), 1);
    assert_eq!(page.size(), 50);
    assert_eq!(page.pages(), 1);
    assert!(!page.has_next());

    // `peers()` attaches no transform, so borrow the raw items instead of
    // cloning via `items()`.
    let peers = page.raw_items();
    assert_eq!(peers.len(), 2);
    let alice = &peers[0];
    assert_eq!(alice.id, "alice");
    assert_eq!(alice.workspace_id, "ws1");
    assert_eq!(alice.created_at, expected_created_at());
    assert!(alice.metadata.is_empty());
    assert!(alice.configuration.is_empty());
    assert_eq!(peers[1].id, "bob");
}

#[tokio::test]
async fn peers_defaults_to_page_one_size_fifty() {
    let server = MockServer::start().await;
    let honcho = common::make_honcho(&server.uri());
    common::mount_workspace_ensure(&server, 1).await;

    // The `query_param` matchers are what actually pin the defaults: the mock
    // only responds to `page=1` & `size=50`, and `.expect(1)` proves exactly
    // one such request was issued (the response body could lie about either).
    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/list"))
        .and(query_param("page", "1"))
        .and(query_param("size", "50"))
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

    let page = honcho.peers().await.unwrap();
    assert_eq!(page.page(), 1);
    assert_eq!(page.size(), 50);
}

#[tokio::test]
async fn peers_reports_has_next_when_more_pages() {
    let server = MockServer::start().await;
    let honcho = common::make_honcho(&server.uri());
    common::mount_workspace_ensure(&server, 1).await;

    // Page 1 of 3 → `has_next()` must be true.
    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/list"))
        .and(query_param("page", "1"))
        .and(query_param("size", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(common::page_json(
            vec![common::peer_response("alice")],
            5,
            1,
            50,
            3,
        )))
        .expect(1)
        .mount(&server)
        .await;

    let page = honcho.peers().await.unwrap();
    assert_eq!(page.pages(), 3);
    assert!(page.has_next());
}

#[tokio::test]
async fn peers_handles_empty_page() {
    let server = MockServer::start().await;
    let honcho = common::make_honcho(&server.uri());
    common::mount_workspace_ensure(&server, 1).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/list"))
        .and(query_param("page", "1"))
        .and(query_param("size", "50"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(common::page_json(vec![], 0, 1, 50, 0)),
        )
        .expect(1)
        .mount(&server)
        .await;

    let page = honcho.peers().await.unwrap();
    assert!(page.raw_items().is_empty());
    assert_eq!(page.total(), 0);
    assert!(!page.has_next());
}

#[tokio::test]
async fn peers_with_filters_sends_body_and_pagination() {
    let server = MockServer::start().await;
    let honcho = common::make_honcho(&server.uri());
    common::mount_workspace_ensure(&server, 1).await;

    let mut filters = HashMap::new();
    filters.insert("role".to_string(), json!("admin"));

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/list"))
        .and(query_param("page", "2"))
        .and(query_param("size", "10"))
        .and(query_param("reverse", "true"))
        .and(body_json(json!({ "filters": { "role": "admin" } })))
        .respond_with(ResponseTemplate::new(200).set_body_json(common::page_json(
            vec![common::peer_response("alice")],
            11,
            2,
            10,
            2,
        )))
        .expect(1)
        .mount(&server)
        .await;

    let page = honcho
        .peers_with_filters(filters, 2, 10, true)
        .await
        .unwrap();
    assert_eq!(page.page(), 2);
    assert_eq!(page.size(), 10);
    assert_eq!(page.raw_items()[0].id, "alice");
}

#[tokio::test]
async fn peers_with_filters_rejects_size_above_max() {
    let server = MockServer::start().await;
    let honcho = common::make_honcho(&server.uri());
    common::mount_workspace_ensure(&server, 1).await;

    // `size = 101` is outside the accepted `1..=100`; the list request must
    // never be issued. `.expect(0)` makes wiremock fail on drop if it is.
    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/list"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;

    let err = honcho
        .peers_with_filters(HashMap::new(), 1, 101, false)
        .await
        .unwrap_err();
    assert!(
        matches!(err, HonchoError::Validation(_)),
        "expected Validation, got {err:?}"
    );
}

#[tokio::test]
async fn peers_maps_404_to_client_error() {
    let server = MockServer::start().await;
    let honcho = common::make_honcho(&server.uri());
    common::mount_workspace_ensure(&server, 1).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/list"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({ "detail": "not found" })))
        .expect(1)
        .mount(&server)
        .await;

    let err = honcho.peers().await.unwrap_err();
    // 404 has a dedicated `NotFound` variant; the catch-all `Client` variant is
    // reserved for 4xx codes without one (405, 413, ...).
    assert!(
        matches!(err, HonchoError::NotFound { .. }),
        "expected NotFound, got {err:?}"
    );
    assert_eq!(err.status_code(), Some(404));
}

#[tokio::test]
async fn peers_maps_500_to_server_error() {
    let server = MockServer::start().await;
    let honcho = common::make_honcho(&server.uri());
    common::mount_workspace_ensure(&server, 1).await;

    // POST is non-idempotent, so the client does not retry the 500: exactly
    // one list request is expected.
    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/list"))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({ "detail": "boom" })))
        .expect(1)
        .mount(&server)
        .await;

    let err = honcho.peers().await.unwrap_err();
    assert!(
        matches!(err, HonchoError::Server { status: 500, .. }),
        "expected Server{{500}}, got {err:?}"
    );
}

#[tokio::test]
async fn peers_malformed_body_is_decode_error() {
    let server = MockServer::start().await;
    let honcho = common::make_honcho(&server.uri());
    common::mount_workspace_ensure(&server, 1).await;

    // 200 OK but the body is not a valid `PageResponse` (missing every field).
    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "unexpected": "shape" })))
        .expect(1)
        .mount(&server)
        .await;

    let err = honcho.peers().await.unwrap_err();
    assert!(
        matches!(err, HonchoError::Decode { .. }),
        "expected Decode, got {err:?}"
    );
}

// ════════════════════════════════════════════════════════════════════════
// sessions()
// ════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn sessions_returns_paginated() {
    let server = MockServer::start().await;
    let honcho = common::make_honcho(&server.uri());
    common::mount_workspace_ensure(&server, 1).await;

    let body = common::page_json(
        vec![
            common::session_response("s1"),
            common::session_response("s2"),
        ],
        2,
        1,
        50,
        1,
    );
    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/sessions/list"))
        .and(query_param("page", "1"))
        .and(query_param("size", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(&server)
        .await;

    let page: Page<SessionResponse> = honcho.sessions().await.unwrap();
    assert_eq!(page.total(), 2);
    assert_eq!(page.page(), 1);
    assert_eq!(page.size(), 50);
    assert!(!page.has_next());

    let sessions = page.raw_items();
    assert_eq!(sessions.len(), 2);
    let s1 = &sessions[0];
    assert_eq!(s1.id, "s1");
    assert_eq!(s1.workspace_id, "ws1");
    assert!(s1.is_active);
    assert_eq!(s1.created_at, expected_created_at());
    assert!(s1.metadata.is_empty());
    assert_eq!(sessions[1].id, "s2");
}

#[tokio::test]
async fn sessions_with_filters_sends_body_and_pagination() {
    let server = MockServer::start().await;
    let honcho = common::make_honcho(&server.uri());
    common::mount_workspace_ensure(&server, 1).await;

    let mut filters = HashMap::new();
    filters.insert("is_active".to_string(), json!(true));

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/sessions/list"))
        .and(query_param("page", "3"))
        .and(query_param("size", "25"))
        .and(body_json(json!({ "filters": { "is_active": true } })))
        .respond_with(ResponseTemplate::new(200).set_body_json(common::page_json(
            vec![common::session_response("s1")],
            60,
            3,
            25,
            3,
        )))
        .expect(1)
        .mount(&server)
        .await;

    let page = honcho
        .sessions_with_filters(filters, 3, 25, false)
        .await
        .unwrap();
    assert_eq!(page.page(), 3);
    assert_eq!(page.size(), 25);
    assert_eq!(page.raw_items()[0].id, "s1");
}

// ════════════════════════════════════════════════════════════════════════
// workspaces()
// ════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn workspaces_returns_ids() {
    let server = MockServer::start().await;
    let honcho = common::make_honcho(&server.uri());
    // `workspaces()` lists across all workspaces and never calls
    // `ensure_workspace`, so no workspace-ensure mock is mounted.

    let body = common::page_json(
        vec![
            common::workspace_response("ws_abc"),
            common::workspace_response("ws_def"),
        ],
        2,
        1,
        50,
        1,
    );
    Mock::given(method("POST"))
        .and(path("/v3/workspaces/list"))
        .and(query_param("page", "1"))
        .and(query_param("size", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(&server)
        .await;

    let page: Page<Workspace, String> = honcho.workspaces().await.unwrap();

    // The attached transform projects each `Workspace` to its `id`.
    let ids = page.items();
    assert_eq!(ids, vec!["ws_abc".to_string(), "ws_def".to_string()]);

    // The untransformed raw items still carry the full typed `Workspace`,
    // including the by-value `created_at`.
    let raw = page.raw_items();
    assert_eq!(raw.len(), 2);
    assert_eq!(raw[0].id, "ws_abc");
    assert_eq!(raw[0].created_at, expected_created_at());
    assert!(raw[0].metadata.is_empty());
}
