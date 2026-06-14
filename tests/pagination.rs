#![allow(clippy::unwrap_used, clippy::expect_used)]
//! F3.2 + F3.3 — `Page` pagination tests.
//!
//! F3.2.x: first page + `next_page()`.
//! F3.3.x: `into_stream()`.
//! Plus `with_fetcher` / transform / accessor-trait coverage.

mod common;

use std::future::Future;
use std::pin::Pin;

use common::{http_client, page_json, peer_fetcher, peer_response};
use honcho_ai::error::HonchoError;
use honcho_ai::types::pagination::{Page, PageResponse};
use honcho_ai::types::peer::Peer;
use serde_json::json;
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Peers-list route every server-backed test targets.
const PEERS_LIST: &str = "/v3/workspaces/ws1/peers/list";

/// Deserializes a [`Peer`] from the shared `peer_response` fixture.
///
/// Sourcing the JSON from the shared helper keeps the Peer wire schema in one
/// place (avoids the type-drift risk of an inline literal).
fn peer(id: &str) -> Peer {
    serde_json::from_value(peer_response(id)).expect("valid Peer fixture")
}

/// Builds a `PageResponse` JSON body wrapping the named peers.
fn peer_page(ids: &[&str], total: u64, page: u64, size: u64, pages: u64) -> serde_json::Value {
    let items = ids.iter().map(|id| peer_response(id)).collect();
    page_json(items, total, page, size, pages)
}

/// Local fetcher that maps any non-success HTTP status to `Server { status }`.
///
/// The shared [`peer_fetcher`] only decodes success bodies, so the 500-path
/// tests keep this status-aware variant. Takes an owned `base` so the returned
/// `impl Fn` is `'static` (no borrowed lifetime to capture).
fn error_fetcher(
    base: String,
) -> impl Fn(u64) -> Pin<Box<dyn Future<Output = honcho_ai::error::Result<PageResponse<Peer>>> + Send>>
{
    move |page_num: u64| {
        let url = format!("{base}{PEERS_LIST}");
        Box::pin(async move {
            let response = http_client()
                .post(url)
                .query(&[("page", page_num)])
                .query(&[("size", 2u64)])
                .header("content-type", "application/json")
                .json(&json!({}))
                .send()
                .await
                .map_err(HonchoError::Transport)?;
            if !response.status().is_success() {
                return Err(HonchoError::Server {
                    status: response.status().as_u16(),
                    message: "server error".into(),
                });
            }
            let pr: PageResponse<Peer> = response.json().await.map_err(HonchoError::Transport)?;
            Ok(pr)
        })
    }
}

/// POSTs page 1 (`page=1&size=N`) with the given JSON body and decodes the
/// response into a `PageResponse<Peer>`.
async fn fetch_page_one(uri: &str, size: u64, body: &serde_json::Value) -> PageResponse<Peer> {
    let response = http_client()
        .post(format!("{uri}{PEERS_LIST}"))
        .query(&[("page", 1u64)])
        .query(&[("size", size)])
        .header("content-type", "application/json")
        .json(body)
        .send()
        .await
        .expect("page 1 request");
    response.json().await.expect("deserialize page 1")
}

// ═══════════════════════════════════════════════════════════════════════
// F3.2.1 — first page exposes items and metadata
// ═══════════════════════════════════════════════════════════════════════
#[tokio::test]
async fn page_first_page_exposes_items_and_metadata() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(PEERS_LIST))
        .and(query_param("page", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(peer_page(
            &["alice", "bob"],
            5,
            1,
            2,
            3,
        )))
        .expect(1)
        .mount(&server)
        .await;

    let page: Page<Peer> =
        Page::from_page_response(fetch_page_one(&server.uri(), 2, &json!({})).await);

    assert_eq!(page.items().len(), 2);
    // `raw_items()` borrows the same data without allocating a new Vec.
    assert_eq!(page.raw_items().len(), 2);
    assert_eq!(page.raw_items()[0].id, "alice");
    assert_eq!(page.total(), 5);
    assert_eq!(page.page(), 1);
    assert_eq!(page.size(), 2);
    assert_eq!(page.pages(), 3);
    assert!(page.has_next());
}

// ═══════════════════════════════════════════════════════════════════════
// F3.2.2 — next_page returns page 2 (PageResponse::with_fetcher no-clone path)
// ═══════════════════════════════════════════════════════════════════════
#[tokio::test]
async fn page_next_page_returns_page_2() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(PEERS_LIST))
        .and(query_param("page", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(peer_page(
            &["alice", "bob"],
            5,
            1,
            2,
            3,
        )))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(PEERS_LIST))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(peer_page(
            &["carol", "dave"],
            5,
            2,
            2,
            3,
        )))
        .expect(1)
        .mount(&server)
        .await;

    let page1_resp = fetch_page_one(&server.uri(), 2, &json!({})).await;
    // No-clone path: attach the fetcher straight onto the `PageResponse`.
    let page1 = page1_resp.with_fetcher(peer_fetcher(&server.uri(), PEERS_LIST, 2, json!({})));

    assert_eq!(page1.items().len(), 2);
    assert_eq!(page1.page(), 1);
    assert!(page1.has_next());

    let page2 = page1
        .next_page()
        .await
        .unwrap()
        .expect("page 2 should exist");
    assert_eq!(page2.items().len(), 2);
    assert_eq!(page2.page(), 2);
    assert_eq!(page2.items()[0].id, "carol");
}

// ═══════════════════════════════════════════════════════════════════════
// F3.2.3 — next_page returns None on the last page (no fetcher attached)
// ═══════════════════════════════════════════════════════════════════════
#[tokio::test]
async fn page_next_page_returns_none_when_no_more() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(PEERS_LIST))
        .and(query_param("page", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(peer_page(&["alice"], 1, 1, 2, 1)))
        .expect(1)
        .mount(&server)
        .await;

    let page: Page<Peer> =
        Page::from_page_response(fetch_page_one(&server.uri(), 2, &json!({})).await);

    assert_eq!(page.items().len(), 1);
    assert!(!page.has_next());
    assert!(page.next_page().await.unwrap().is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// F3.2.4 — next_page propagates the request body to subsequent pages
// ═══════════════════════════════════════════════════════════════════════
#[tokio::test]
async fn page_propagates_filters_on_subsequent_pages() {
    let server = MockServer::start().await;
    let filter_body = json!({ "metadata": { "role": "admin" } });

    Mock::given(method("POST"))
        .and(path(PEERS_LIST))
        .and(query_param("page", "1"))
        .and(body_json(&filter_body))
        .respond_with(ResponseTemplate::new(200).set_body_json(peer_page(&["alice"], 2, 1, 1, 2)))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(PEERS_LIST))
        .and(query_param("page", "2"))
        .and(body_json(&filter_body))
        .respond_with(ResponseTemplate::new(200).set_body_json(peer_page(&["bob"], 2, 2, 1, 2)))
        .expect(1)
        .mount(&server)
        .await;

    let page1_resp = fetch_page_one(&server.uri(), 1, &filter_body).await;
    let page1 = page1_resp.with_fetcher(peer_fetcher(
        &server.uri(),
        PEERS_LIST,
        1,
        filter_body.clone(),
    ));

    let page2 = page1
        .next_page()
        .await
        .unwrap()
        .expect("page 2 should exist");
    assert_eq!(page2.items().len(), 1);
    assert_eq!(page2.page(), 2);
    assert_eq!(page2.items()[0].id, "bob");
}

// ═══════════════════════════════════════════════════════════════════════
// F3.2.5 — next_page propagates the reverse query param
// ═══════════════════════════════════════════════════════════════════════
#[tokio::test]
async fn page_propagates_reverse_query_on_subsequent_pages() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(PEERS_LIST))
        .and(query_param("page", "1"))
        .and(query_param("reverse", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(peer_page(&["bob"], 2, 1, 1, 2)))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(PEERS_LIST))
        .and(query_param("page", "2"))
        .and(query_param("reverse", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(peer_page(&["alice"], 2, 2, 1, 2)))
        .expect(1)
        .mount(&server)
        .await;

    let response = http_client()
        .post(format!("{}{PEERS_LIST}", server.uri()))
        .query(&[("page", 1u64)])
        .query(&[("size", 1u64)])
        .query(&[("reverse", "true")])
        .header("content-type", "application/json")
        .json(&json!({}))
        .send()
        .await
        .expect("page 1 request");
    let page1_resp: PageResponse<Peer> = response.json().await.expect("deserialize page 1");

    let server_uri = server.uri();
    // The shared `peer_fetcher` never emits `reverse`, so this test keeps a
    // local closure that appends `reverse=true` to every refetch.
    let page1 = page1_resp.with_fetcher(move |page_num: u64| {
        let uri = server_uri.clone();
        Box::pin(async move {
            let response = http_client()
                .post(format!("{uri}{PEERS_LIST}"))
                .query(&[("page", page_num)])
                .query(&[("size", 1u64)])
                .query(&[("reverse", "true")])
                .header("content-type", "application/json")
                .json(&json!({}))
                .send()
                .await
                .map_err(HonchoError::Transport)?;
            let pr: PageResponse<Peer> = response.json().await.map_err(HonchoError::Transport)?;
            Ok(pr)
        })
    });

    let page2 = page1
        .next_page()
        .await
        .unwrap()
        .expect("page 2 should exist");
    assert_eq!(page2.items().len(), 1);
    assert_eq!(page2.page(), 2);
    assert_eq!(page2.items()[0].id, "alice");
}

// ═══════════════════════════════════════════════════════════════════════
// F3.3.1 — into_stream yields all items across pages
// ═══════════════════════════════════════════════════════════════════════
#[tokio::test]
async fn page_into_stream_yields_all_items_across_pages() {
    use futures_util::TryStreamExt;

    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(PEERS_LIST))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(peer_page(
            &["carol", "dave"],
            6,
            2,
            2,
            3,
        )))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(PEERS_LIST))
        .and(query_param("page", "3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(peer_page(
            &["eve", "frank"],
            6,
            3,
            2,
            3,
        )))
        .expect(1)
        .mount(&server)
        .await;

    let page1_resp = PageResponse::<Peer>::new(vec![peer("alice"), peer("bob")], 6, 1, 2, 3);
    let page1 = page1_resp.with_fetcher(peer_fetcher(&server.uri(), PEERS_LIST, 2, json!({})));

    let all: Vec<Peer> = page1
        .into_stream()
        .try_collect()
        .await
        .expect("collect all items");

    let ids: Vec<&str> = all.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["alice", "bob", "carol", "dave", "eve", "frank"]);
}

// ═══════════════════════════════════════════════════════════════════════
// F3.3.2 — into_stream emits page-1 items, then surfaces the page-2 500 error
// ═══════════════════════════════════════════════════════════════════════
#[tokio::test]
async fn page_into_stream_propagates_error_on_2nd_page() {
    use futures_util::StreamExt;

    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(PEERS_LIST))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({ "error": "internal" })))
        .expect(1)
        .mount(&server)
        .await;

    let page1_resp = PageResponse::<Peer>::new(vec![peer("alice"), peer("bob")], 4, 1, 2, 2);
    let page1 = page1_resp.with_fetcher(error_fetcher(server.uri()));

    let mut stream = Box::pin(page1.into_stream());

    // Page-1 items are emitted *before* the failing page-2 fetch.
    let first = stream.next().await.expect("item 1").expect("alice ok");
    assert_eq!(first.id, "alice");
    let second = stream.next().await.expect("item 2").expect("bob ok");
    assert_eq!(second.id, "bob");

    // Page 2 → HTTP 500 → mapped to Server { status: 500 }.
    let third = stream.next().await.expect("error slot");
    let is_server_500 = matches!(third, Err(HonchoError::Server { status: 500, .. }));
    assert!(
        is_server_500,
        "expected Server {{ status: 500 }}, got {third:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// F3.3.3 — dropping the stream mid-way does not fetch the next page
// ═══════════════════════════════════════════════════════════════════════
#[tokio::test]
async fn page_into_stream_drop_in_middle_does_not_fetch_next() {
    use futures_util::StreamExt;

    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(PEERS_LIST))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(peer_page(
            &["carol", "dave"],
            4,
            2,
            2,
            2,
        )))
        .expect(0)
        .mount(&server)
        .await;

    let page1_resp = PageResponse::<Peer>::new(vec![peer("alice"), peer("bob")], 4, 1, 2, 2);
    let page1 = page1_resp.with_fetcher(peer_fetcher(&server.uri(), PEERS_LIST, 2, json!({})));

    let mut stream = Box::pin(page1.into_stream());
    assert!(stream.next().await.is_some());
    drop(stream);

    let requests = server.received_requests().await.unwrap();
    assert_eq!(
        requests.len(),
        0,
        "page 2 should not be fetched after dropping stream"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// P1.8 — next_page returns Err (Server 500), with page-1 items still readable
// ═══════════════════════════════════════════════════════════════════════
#[tokio::test]
async fn page_next_page_returns_err_on_http_500() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(PEERS_LIST))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({ "error": "internal" })))
        .expect(1)
        .mount(&server)
        .await;

    let page1_resp = PageResponse::<Peer>::new(vec![peer("alice"), peer("bob")], 4, 1, 2, 2);
    let page1 = page1_resp.with_fetcher(error_fetcher(server.uri()));

    assert!(page1.has_next());
    // Page-1 items remain readable before attempting the failing page-2 fetch.
    assert_eq!(page1.items().len(), 2);
    assert_eq!(page1.items()[0].id, "alice");

    let err = page1.next_page().await.unwrap_err();
    assert!(
        matches!(err, HonchoError::Server { status: 500, .. }),
        "expected Server {{ status: 500 }}, got {err:?}"
    );
    assert_eq!(err.status_code(), Some(500));
}

// ═══════════════════════════════════════════════════════════════════════
// No fetcher: next_page is Ok(None) and into_stream yields only the current page
// ═══════════════════════════════════════════════════════════════════════
#[tokio::test]
async fn page_without_fetcher_has_no_next_and_stream_yields_current_only() {
    use futures_util::TryStreamExt;

    // `has_next()` is true, but with no fetcher `next_page` resolves to Ok(None).
    let page: Page<Peer> = Page::new(vec![peer("alice")], 4, 1, 2, 2);
    assert!(page.has_next());
    assert!(page.next_page().await.unwrap().is_none());

    // `into_stream` without a fetcher yields only the current page's items.
    let page: Page<Peer> = Page::new(vec![peer("alice"), peer("bob")], 4, 1, 2, 2);
    let items: Vec<Peer> = page.into_stream().try_collect().await.unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].id, "alice");
}

// ═══════════════════════════════════════════════════════════════════════
// Non-increasing page-number guard: a stuck server must not loop forever
// ═══════════════════════════════════════════════════════════════════════
#[tokio::test]
async fn page_into_stream_stops_on_non_increasing_page_number() {
    use futures_util::TryStreamExt;

    // A misbehaving fetcher that always reports `page = 1` (never advances).
    // The stream must terminate via the non-increasing-page guard rather than
    // refetching forever.
    let stuck = |_page_num: u64| {
        Box::pin(async move { Ok(PageResponse::<Peer>::new(vec![peer("stuck")], 9, 1, 1, 3)) })
            as Pin<Box<dyn Future<Output = honcho_ai::error::Result<PageResponse<Peer>>> + Send>>
    };
    let page1 = PageResponse::<Peer>::new(vec![peer("alice")], 9, 1, 1, 3).with_fetcher(stuck);

    let items: Vec<Peer> = page1.into_stream().try_collect().await.unwrap();
    // page-1 item + exactly one fetched (stuck) page, then the guard breaks.
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].id, "alice");
    assert_eq!(items[1].id, "stuck");
}

// ═══════════════════════════════════════════════════════════════════════
// map() must propagate to every page the stream fetches, not just page 1
// ═══════════════════════════════════════════════════════════════════════
#[tokio::test]
async fn page_transform_propagates_to_refetched_pages() {
    use futures_util::TryStreamExt;

    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(PEERS_LIST))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(peer_page(
            &["carol", "dave"],
            4,
            2,
            2,
            2,
        )))
        .expect(1)
        .mount(&server)
        .await;

    let page1_resp = PageResponse::<Peer>::new(vec![peer("alice"), peer("bob")], 4, 1, 2, 2);
    let page1 = page1_resp.with_fetcher(peer_fetcher(&server.uri(), PEERS_LIST, 2, json!({})));

    let ids: Vec<String> = page1
        .map(|p| p.id)
        .into_stream()
        .try_collect()
        .await
        .unwrap();
    assert_eq!(ids, ["alice", "bob", "carol", "dave"]);
}

// ═══════════════════════════════════════════════════════════════════════
// Page::with_fetcher no-clone path (sole-owner Arc) still enables next_page
// ═══════════════════════════════════════════════════════════════════════
#[tokio::test]
async fn page_with_fetcher_no_clone_path_enables_next_page() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(PEERS_LIST))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(peer_page(&["bob"], 2, 2, 1, 2)))
        .expect(1)
        .mount(&server)
        .await;

    // `Page::new` yields a sole-owner Arc, so `with_fetcher` takes the no-clone
    // (`Arc::try_unwrap` Ok) branch.
    let page1: Page<Peer> = Page::new(vec![peer("alice")], 2, 1, 1, 2).with_fetcher(peer_fetcher(
        &server.uri(),
        PEERS_LIST,
        1,
        json!({}),
    ));

    assert!(page1.has_next());
    let page2 = page1.next_page().await.unwrap().expect("page 2");
    assert_eq!(page2.items()[0].id, "bob");
}

// ═══════════════════════════════════════════════════════════════════════
// F3.x — Page::map transforms items (raw items left intact)
// ═══════════════════════════════════════════════════════════════════════
#[tokio::test]
async fn page_map_transforms_items() {
    let page = Page::new(vec![peer("alice"), peer("bob")], 2, 1, 50, 1);

    let mapped: Page<Peer, String> = page.map(|p| p.id);

    assert_eq!(mapped.items(), vec!["alice".to_string(), "bob".to_string()]);
    assert_eq!(mapped.total(), 2);
    assert_eq!(mapped.page(), 1);
    // The transform leaves the raw items untouched.
    assert_eq!(mapped.raw_items().len(), 2);
    assert_eq!(mapped.raw_items()[0].id, "alice");
}

// ═══════════════════════════════════════════════════════════════════════
// items_ref / into_items expose the raw items without the transform path
// ═══════════════════════════════════════════════════════════════════════
#[test]
fn page_items_ref_and_into_items_expose_raw_items() {
    let page: Page<Peer> = Page::new(vec![peer("alice"), peer("bob")], 2, 1, 50, 1);
    // `items_ref` borrows the raw slice without allocating.
    assert_eq!(page.items_ref().len(), 2);
    assert_eq!(page.items_ref()[0].id, "alice");

    // `into_items` moves the raw items out (sole-owner ⇒ no clone).
    let items = page.into_items();
    assert_eq!(items.len(), 2);
    assert_eq!(items[1].id, "bob");
}

// ═══════════════════════════════════════════════════════════════════════
// Default is an empty, first-page page with no fetcher
// ═══════════════════════════════════════════════════════════════════════
#[test]
fn page_default_is_empty_first_page() {
    let page: Page<Peer> = Page::default();
    assert!(page.items_ref().is_empty());
    assert_eq!(page.total(), 0);
    assert_eq!(page.page(), 1);
    assert_eq!(page.size(), 0);
    assert_eq!(page.pages(), 0);
    assert!(!page.has_next());
}

// ═══════════════════════════════════════════════════════════════════════
// Serialize emits the wire shape and round-trips through Deserialize + PartialEq
// ═══════════════════════════════════════════════════════════════════════
#[test]
fn page_serialize_matches_wire_shape_and_roundtrips() {
    let page: Page<Peer> = Page::new(vec![peer("alice")], 1, 1, 50, 1);

    let value = serde_json::to_value(&page).unwrap();
    let expected_item = serde_json::to_value(peer("alice")).unwrap();
    assert_eq!(
        value,
        json!({
            "items": [expected_item],
            "total": 1,
            "page": 1,
            "size": 50,
            "pages": 1
        })
    );

    // Deserialize round-trip yields an equal page (PartialEq compares raw items
    // + metadata, ignoring the fetcher/transform).
    let back: Page<Peer> = serde_json::from_value(value).unwrap();
    assert_eq!(page, back);
}

// ═══════════════════════════════════════════════════════════════════════
// PartialEq distinguishes differing metadata
// ═══════════════════════════════════════════════════════════════════════
#[test]
fn page_partial_eq_distinguishes_metadata() {
    let a: Page<Peer> = Page::new(vec![peer("alice")], 1, 1, 50, 1);
    let b: Page<Peer> = Page::new(vec![peer("alice")], 1, 1, 50, 1);
    let c: Page<Peer> = Page::new(vec![peer("alice")], 2, 1, 50, 1);
    assert_eq!(a, b);
    assert_ne!(a, c);
}

// ═══════════════════════════════════════════════════════════════════════
// PR5 — page-number overflow at u64::MAX must not panic
//
// `has_next()` is `page < pages`, so it is never true at `page == u64::MAX`.
// The reachable hazard was the `page + 1` in `into_stream` (plus the guard in
// `next_page`): both must treat overflow as "no more pages".
// ═══════════════════════════════════════════════════════════════════════
#[tokio::test]
async fn page_max_page_number_does_not_overflow() {
    use futures_util::TryStreamExt;

    let page_resp = PageResponse::<Peer>::new(vec![peer("alice")], 1, u64::MAX, 2, u64::MAX);

    // A fetcher that errors if ever invoked: at the u64::MAX boundary no further
    // page should be requested.
    let page: Page<Peer> = page_resp.with_fetcher(|_page_num: u64| {
        Box::pin(async move {
            Err::<PageResponse<Peer>, HonchoError>(HonchoError::Validation(
                "fetcher must not be called past u64::MAX".into(),
            ))
        })
    });

    // `next_page()` must not panic and must report "no more pages".
    assert!(page.next_page().await.unwrap().is_none());

    // `into_stream()` must terminate and yield only the current page's items.
    let collected: Vec<Peer> = page.into_stream().try_collect().await.unwrap();
    assert_eq!(collected.len(), 1);
    assert_eq!(collected[0].id, "alice");
}
