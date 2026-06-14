//! Wire tests for `Peer::sessions` and `Peer::sessions_with_options`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::needless_borrows_for_generic_args,
    missing_docs
)]

mod common;

use std::collections::HashMap;

use common::{make_honcho, mount_workspace_ensure, page_json, peer_response, session_response};
use honcho_ai::error::HonchoError;
use honcho_ai::types::pagination::Page;
use honcho_ai::types::session::{SessionListOptions, SessionResponse};
use wiremock::matchers::{
    body_bytes, body_json, method, path, query_param, query_param_is_missing,
};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Sessions-list route every test below targets.
const SESSIONS_PATH: &str = "/v3/workspaces/ws1/peers/alice/sessions";

/// Mounts the peer get-or-create POST that `peer("alice").build()` issues.
///
/// The shared helpers cover the workspace-ensure POST and the peer JSON body
/// ([`peer_response`]), but not the peer-ensure mock itself, so this stays
/// local. Verifies exactly one call.
async fn mount_peer(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers"))
        .and(body_json(serde_json::json!({ "id": "alice" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(peer_response("alice")))
        .expect(1)
        .mount(server)
        .await;
}

/// Mounts both ensure POSTs (workspace + peer) that precede every sessions call.
async fn mount_workspace_and_peer(server: &MockServer) {
    mount_workspace_ensure(server, 1).await;
    mount_peer(server).await;
}

#[tokio::test]
async fn peer_sessions_defaults_no_body() {
    let server = MockServer::start().await;
    let honcho = make_honcho(&server.uri());
    mount_workspace_and_peer(&server).await;

    let body = page_json(vec![session_response("s1")], 1, 1, 50, 1);

    Mock::given(method("POST"))
        .and(path(SESSIONS_PATH))
        .and(query_param("page", "1"))
        .and(query_param("size", "50"))
        // `sessions()` passes `reverse = false`, so the param must be absent…
        .and(query_param_is_missing("reverse"))
        // …and `body = None` must serialize to an empty request body.
        .and(body_bytes(Vec::<u8>::new()))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(&server)
        .await;

    let peer = honcho.peer("alice").build().await.unwrap();
    let page: Page<SessionResponse> = peer.sessions().await.unwrap();

    assert_eq!(page.items().len(), 1);
    assert_eq!(page.items()[0].id, "s1");
    assert_eq!(page.total(), 1);
    assert_eq!(page.page(), 1);
    assert_eq!(page.size(), 50);
    assert_eq!(page.pages(), 1);
    assert!(!page.has_next());
}

#[tokio::test]
async fn peer_sessions_with_options_sends_filters_and_pagination() {
    let server = MockServer::start().await;
    let honcho = make_honcho(&server.uri());
    mount_workspace_and_peer(&server).await;

    let body = page_json(vec![session_response("s2")], 1, 2, 10, 1);

    Mock::given(method("POST"))
        .and(path(SESSIONS_PATH))
        .and(body_json(
            serde_json::json!({ "filters": { "is_active": true } }),
        ))
        .and(query_param("page", "2"))
        .and(query_param("size", "10"))
        .and(query_param("reverse", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(&server)
        .await;

    let peer = honcho.peer("alice").build().await.unwrap();
    let opts = SessionListOptions::builder()
        .filters(HashMap::from([(
            "is_active".to_string(),
            serde_json::json!(true),
        )]))
        .page(2)
        .size(10)
        .reverse(true)
        .build();
    let page: Page<SessionResponse> = peer.sessions_with_options(&opts).await.unwrap();

    assert_eq!(page.items().len(), 1);
    assert_eq!(page.items()[0].id, "s2");
    assert_eq!(page.total(), 1);
    assert_eq!(page.page(), 2);
    assert_eq!(page.size(), 10);
    assert_eq!(page.pages(), 1);
    assert!(!page.has_next());
}

#[tokio::test]
async fn peer_sessions_with_options_minimal_body() {
    let server = MockServer::start().await;
    let honcho = make_honcho(&server.uri());
    mount_workspace_and_peer(&server).await;

    let body = page_json(vec![], 0, 1, 50, 0);

    Mock::given(method("POST"))
        .and(path(SESSIONS_PATH))
        .and(query_param("page", "1"))
        .and(query_param("size", "50"))
        // `filters = None` ⇒ `body = None` ⇒ empty body, and `reverse` defaults
        // to false ⇒ the query param is absent.
        .and(query_param_is_missing("reverse"))
        .and(body_bytes(Vec::<u8>::new()))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(&server)
        .await;

    let peer = honcho.peer("alice").build().await.unwrap();
    let opts = SessionListOptions::builder().build();
    let page: Page<SessionResponse> = peer.sessions_with_options(&opts).await.unwrap();

    assert_eq!(page.items().len(), 0);
    assert_eq!(page.total(), 0);
    assert_eq!(page.page(), 1);
    assert_eq!(page.size(), 50);
    assert_eq!(page.pages(), 0);
    assert!(!page.has_next());
}

/// Explicit `reverse(false)` must behave like the default: no `reverse` query
/// param on the wire.
#[tokio::test]
async fn peer_sessions_with_options_reverse_false_omits_param() {
    let server = MockServer::start().await;
    let honcho = make_honcho(&server.uri());
    mount_workspace_and_peer(&server).await;

    let body = page_json(vec![session_response("s3")], 1, 1, 50, 1);

    Mock::given(method("POST"))
        .and(path(SESSIONS_PATH))
        .and(query_param("page", "1"))
        .and(query_param("size", "50"))
        .and(query_param_is_missing("reverse"))
        .and(body_bytes(Vec::<u8>::new()))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(&server)
        .await;

    let peer = honcho.peer("alice").build().await.unwrap();
    let opts = SessionListOptions::builder().reverse(false).build();
    let page: Page<SessionResponse> = peer.sessions_with_options(&opts).await.unwrap();

    assert_eq!(page.items()[0].id, "s3");
    assert!(!page.has_next());
}

/// A multi-key filter map must serialize into the request body in full.
#[tokio::test]
async fn peer_sessions_with_options_multi_key_filters() {
    let server = MockServer::start().await;
    let honcho = make_honcho(&server.uri());
    mount_workspace_and_peer(&server).await;

    let body = page_json(vec![session_response("s4")], 1, 1, 50, 1);

    // `body_json` compares the full JSON object, which is key-order independent,
    // so the multi-key filter matches regardless of `HashMap` iteration order.
    Mock::given(method("POST"))
        .and(path(SESSIONS_PATH))
        .and(body_json(serde_json::json!({
            "filters": { "is_active": true, "topic": "rust" }
        })))
        .and(query_param("page", "1"))
        .and(query_param("size", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(&server)
        .await;

    let peer = honcho.peer("alice").build().await.unwrap();
    let opts = SessionListOptions::builder()
        .filters(HashMap::from([
            ("is_active".to_string(), serde_json::json!(true)),
            ("topic".to_string(), serde_json::json!("rust")),
        ]))
        .build();
    let page: Page<SessionResponse> = peer.sessions_with_options(&opts).await.unwrap();

    assert_eq!(page.items()[0].id, "s4");
}

/// Exercises `paginate_post`'s attached fetcher end-to-end: the page-2 fetch
/// must re-issue the same body and `size`, only bumping `page`.
#[tokio::test]
async fn peer_sessions_multi_page_propagates_query_and_body() {
    let server = MockServer::start().await;
    let honcho = make_honcho(&server.uri());
    mount_workspace_and_peer(&server).await;

    let filter_body = serde_json::json!({ "filters": { "is_active": true } });
    let page1 = page_json(vec![session_response("s1")], 2, 1, 1, 2);
    let page2 = page_json(vec![session_response("s2")], 2, 2, 1, 2);

    // Page 1: the initial `sessions_with_options` POST.
    Mock::given(method("POST"))
        .and(path(SESSIONS_PATH))
        .and(query_param("page", "1"))
        .and(query_param("size", "1"))
        .and(body_json(&filter_body))
        .respond_with(ResponseTemplate::new(200).set_body_json(page1))
        .expect(1)
        .mount(&server)
        .await;

    // Page 2: driven by the fetcher `paginate_post` attaches.
    Mock::given(method("POST"))
        .and(path(SESSIONS_PATH))
        .and(query_param("page", "2"))
        .and(query_param("size", "1"))
        .and(body_json(&filter_body))
        .respond_with(ResponseTemplate::new(200).set_body_json(page2))
        .expect(1)
        .mount(&server)
        .await;

    let peer = honcho.peer("alice").build().await.unwrap();
    let opts = SessionListOptions::builder()
        .filters(HashMap::from([(
            "is_active".to_string(),
            serde_json::json!(true),
        )]))
        .page(1)
        .size(1)
        .build();
    let first: Page<SessionResponse> = peer.sessions_with_options(&opts).await.unwrap();

    assert_eq!(first.items()[0].id, "s1");
    assert_eq!(first.page(), 1);
    assert!(first.has_next());

    let second = first
        .next_page()
        .await
        .unwrap()
        .expect("page 2 should exist");
    assert_eq!(second.items()[0].id, "s2");
    assert_eq!(second.page(), 2);
    assert!(!second.has_next());
}

/// `validate_pagination` rejects bad page/size before any HTTP request, with a
/// `Validation` error carrying no HTTP status. No sessions mock is mounted, so
/// a leaked request would 404 and surface as a non-`Validation` error.
#[tokio::test]
async fn peer_sessions_with_options_rejects_invalid_pagination() {
    let server = MockServer::start().await;
    let honcho = make_honcho(&server.uri());
    mount_workspace_and_peer(&server).await;

    let peer = honcho.peer("alice").build().await.unwrap();

    for (page, size) in [(0_u64, 50_u64), (1, 0), (1, 101)] {
        let opts = SessionListOptions::builder().page(page).size(size).build();
        let err = peer.sessions_with_options(&opts).await.unwrap_err();
        assert!(
            matches!(err, HonchoError::Validation(_)),
            "page={page} size={size} should be a Validation error, got {err:?}"
        );
        assert!(err.status_code().is_none());
    }
}
