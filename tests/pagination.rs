#![allow(clippy::unwrap_used, clippy::expect_used)]
//! F3.2 + F3.3 — Page pagination tests
//!
//! F3.2.x: Tests for `Page<TRaw, TOut>` first page + `next_page()`.
//! F3.3.x: Tests for `Page::into_stream()`.

use honcho_ai::error::HonchoError;
use honcho_ai::types::pagination::{Page, PageResponse};
use honcho_ai::types::peer::Peer;

fn peer_json(id: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "workspace_id": "ws1",
        "created_at": "2025-01-15T10:30:00Z",
        "metadata": {},
        "configuration": {}
    })
}

fn page_json(item_ids: &[&str], total: u64, page: u64, size: u64, pages: u64) -> serde_json::Value {
    serde_json::json!({
        "items": item_ids.iter().map(|id| peer_json(id)).collect::<Vec<_>>(),
        "total": total,
        "page": page,
        "size": size,
        "pages": pages
    })
}

// ═══════════════════════════════════════════════════════════════════════
// F3.2.1 — first page exposes items and metadata
// ═══════════════════════════════════════════════════════════════════════
#[tokio::test]
async fn page_first_page_exposes_items_and_metadata() {
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;

    let page1_body = page_json(&["alice", "bob"], 5, 1, 2, 3);

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/list"))
        .and(query_param("page", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page1_body))
        .mount(&server)
        .await;

    let resp = reqwest::Client::new()
        .post(format!(
            "{}{}",
            server.uri(),
            "/v3/workspaces/ws1/peers/list?page=1&size=2"
        ))
        .header("content-type", "application/json")
        .json(&serde_json::json!({}))
        .send()
        .await
        .expect("HTTP request");

    let page: Page<Peer> = resp.json().await.expect("deserialize page");

    assert_eq!(page.items().len(), 2);
    assert_eq!(page.total(), 5);
    assert_eq!(page.pages(), 3);
    assert!(page.has_next());
}

// ═══════════════════════════════════════════════════════════════════════
// F3.2.2 — next_page returns page 2
// ═══════════════════════════════════════════════════════════════════════
#[tokio::test]
async fn page_next_page_returns_page_2() {
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;

    let page1_body = page_json(&["alice", "bob"], 5, 1, 2, 3);
    let page2_body = page_json(&["carol", "dave"], 5, 2, 2, 3);

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/list"))
        .and(query_param("page", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page1_body))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/list"))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page2_body))
        .mount(&server)
        .await;

    let resp = reqwest::Client::new()
        .post(format!(
            "{}{}",
            server.uri(),
            "/v3/workspaces/ws1/peers/list?page=1&size=2"
        ))
        .header("content-type", "application/json")
        .json(&serde_json::json!({}))
        .send()
        .await
        .expect("page 1 request");

    let page1_resp: PageResponse<Peer> = resp.json().await.expect("deserialize page 1");
    let server_uri = server.uri();

    let page1 = Page::from_page_response(page1_resp).with_fetcher(move |page_num: u64| {
        let uri = server_uri.clone();
        Box::pin(async move {
            let response = reqwest::Client::new()
                .post(format!("{}{}", uri, "/v3/workspaces/ws1/peers/list"))
                .query(&[("page", page_num)])
                .query(&[("size", 2u64)])
                .header("content-type", "application/json")
                .json(&serde_json::json!({}))
                .send()
                .await
                .map_err(HonchoError::Transport)?;
            let pr: PageResponse<Peer> = response.json().await.map_err(HonchoError::Transport)?;
            Ok(pr)
        })
    });

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
// F3.2.3 — next_page returns None when no more pages
// ═══════════════════════════════════════════════════════════════════════
#[tokio::test]
async fn page_next_page_returns_none_when_no_more() {
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;

    let page_body = page_json(&["alice"], 1, 1, 2, 1);

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/list"))
        .and(query_param("page", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page_body))
        .mount(&server)
        .await;

    let resp = reqwest::Client::new()
        .post(format!(
            "{}{}",
            server.uri(),
            "/v3/workspaces/ws1/peers/list?page=1&size=2"
        ))
        .header("content-type", "application/json")
        .json(&serde_json::json!({}))
        .send()
        .await
        .expect("page request");

    let page: Page<Peer> = resp.json().await.expect("deserialize page");

    assert_eq!(page.items().len(), 1);
    assert!(!page.has_next());
    assert!(page.next_page().await.unwrap().is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// F3.2.4 — next_page propagates filters from original request body
// ═══════════════════════════════════════════════════════════════════════
#[tokio::test]
async fn page_propagates_filters_on_subsequent_pages() {
    use wiremock::matchers::{body_json, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;

    let filter_body = serde_json::json!({"metadata": {"role": "admin"}});
    let page1_body = page_json(&["alice"], 2, 1, 1, 2);
    let page2_body = page_json(&["bob"], 2, 2, 1, 2);

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/list"))
        .and(query_param("page", "1"))
        .and(body_json(&filter_body))
        .respond_with(ResponseTemplate::new(200).set_body_json(page1_body))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/list"))
        .and(query_param("page", "2"))
        .and(body_json(&filter_body))
        .respond_with(ResponseTemplate::new(200).set_body_json(page2_body))
        .expect(1)
        .mount(&server)
        .await;

    let resp = reqwest::Client::new()
        .post(format!(
            "{}{}",
            server.uri(),
            "/v3/workspaces/ws1/peers/list?page=1&size=1"
        ))
        .header("content-type", "application/json")
        .json(&filter_body)
        .send()
        .await
        .expect("page 1 request");

    let page1_resp: PageResponse<Peer> = resp.json().await.expect("deserialize page 1");
    let server_uri = server.uri();
    let captured_body = filter_body.clone();

    let page1 = Page::from_page_response(page1_resp).with_fetcher(move |page_num: u64| {
        let uri = server_uri.clone();
        let body = captured_body.clone();
        Box::pin(async move {
            let response = reqwest::Client::new()
                .post(format!("{}{}", uri, "/v3/workspaces/ws1/peers/list"))
                .query(&[("page", page_num)])
                .query(&[("size", 1u64)])
                .header("content-type", "application/json")
                .json(&body)
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
    assert_eq!(page2.items()[0].id, "bob");
}

// ═══════════════════════════════════════════════════════════════════════
// F3.2.5 — next_page propagates reverse query param
// ═══════════════════════════════════════════════════════════════════════
#[tokio::test]
async fn page_propagates_reverse_query_on_subsequent_pages() {
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;

    let page1_body = page_json(&["bob"], 2, 1, 1, 2);
    let page2_body = page_json(&["alice"], 2, 2, 1, 2);

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/list"))
        .and(query_param("page", "1"))
        .and(query_param("reverse", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page1_body))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/list"))
        .and(query_param("page", "2"))
        .and(query_param("reverse", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page2_body))
        .expect(1)
        .mount(&server)
        .await;

    let resp = reqwest::Client::new()
        .post(format!(
            "{}{}",
            server.uri(),
            "/v3/workspaces/ws1/peers/list?page=1&size=1&reverse=true"
        ))
        .header("content-type", "application/json")
        .json(&serde_json::json!({}))
        .send()
        .await
        .expect("page 1 request");

    let page1_resp: PageResponse<Peer> = resp.json().await.expect("deserialize page 1");
    let server_uri = server.uri();

    let page1 = Page::from_page_response(page1_resp).with_fetcher(move |page_num: u64| {
        let uri = server_uri.clone();
        Box::pin(async move {
            let response = reqwest::Client::new()
                .post(format!("{}{}", uri, "/v3/workspaces/ws1/peers/list"))
                .query(&[("page", page_num)])
                .query(&[("size", 1u64)])
                .query(&[("reverse", "true")])
                .header("content-type", "application/json")
                .json(&serde_json::json!({}))
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
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;

    let page2_body = page_json(&["carol", "dave"], 6, 2, 2, 3);
    let page3_body = page_json(&["eve", "frank"], 6, 3, 2, 3);

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/list"))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page2_body))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/list"))
        .and(query_param("page", "3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page3_body))
        .mount(&server)
        .await;

    let page1_resp = PageResponse::<Peer>::new(
        vec![
            serde_json::from_value(peer_json("alice")).unwrap(),
            serde_json::from_value(peer_json("bob")).unwrap(),
        ],
        6,
        1,
        2,
        3,
    );
    let server_uri = server.uri();

    let page1 = Page::from_page_response(page1_resp).with_fetcher(move |page_num: u64| {
        let uri = server_uri.clone();
        Box::pin(async move {
            let response = reqwest::Client::new()
                .post(format!("{}{}", uri, "/v3/workspaces/ws1/peers/list"))
                .query(&[("page", page_num)])
                .query(&[("size", 2u64)])
                .header("content-type", "application/json")
                .json(&serde_json::json!({}))
                .send()
                .await
                .map_err(HonchoError::Transport)?;
            let pr: PageResponse<Peer> = response.json().await.map_err(HonchoError::Transport)?;
            Ok(pr)
        })
    });

    let all: Vec<Peer> = page1
        .into_stream()
        .try_collect()
        .await
        .expect("collect all items");

    assert_eq!(all.len(), 6);
    assert_eq!(all[0].id, "alice");
    assert_eq!(all[1].id, "bob");
    assert_eq!(all[2].id, "carol");
    assert_eq!(all[3].id, "dave");
    assert_eq!(all[4].id, "eve");
    assert_eq!(all[5].id, "frank");
}

// ═══════════════════════════════════════════════════════════════════════
// F3.3.2 — into_stream propagates error on 2nd page
// ═══════════════════════════════════════════════════════════════════════
#[tokio::test]
async fn page_into_stream_propagates_error_on_2nd_page() {
    use futures_util::TryStreamExt;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/list"))
        .and(query_param("page", "2"))
        .respond_with(
            ResponseTemplate::new(500).set_body_json(serde_json::json!({"error": "internal"})),
        )
        .mount(&server)
        .await;

    let page1_resp = PageResponse::<Peer>::new(
        vec![
            serde_json::from_value(peer_json("alice")).unwrap(),
            serde_json::from_value(peer_json("bob")).unwrap(),
        ],
        4,
        1,
        2,
        2,
    );
    let server_uri = server.uri();

    let page1 = Page::from_page_response(page1_resp).with_fetcher(move |page_num: u64| {
        let uri = server_uri.clone();
        Box::pin(async move {
            let response = reqwest::Client::new()
                .post(format!("{}{}", uri, "/v3/workspaces/ws1/peers/list"))
                .query(&[("page", page_num)])
                .query(&[("size", 2u64)])
                .header("content-type", "application/json")
                .json(&serde_json::json!({}))
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
    });

    let result: Result<Vec<Peer>, _> = page1.into_stream().try_collect().await;
    assert!(result.is_err(), "stream should return error on page 2");
}

// ═══════════════════════════════════════════════════════════════════════
// F3.3.3 — into_stream drop in middle does not fetch next
#[tokio::test]
async fn page_into_stream_drop_in_middle_does_not_fetch_next() {
    use futures_util::StreamExt;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;

    let page2_body = page_json(&["carol", "dave"], 4, 2, 2, 2);

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/list"))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page2_body))
        .mount(&server)
        .await;

    let page1_resp = PageResponse::<Peer>::new(
        vec![
            serde_json::from_value(peer_json("alice")).unwrap(),
            serde_json::from_value(peer_json("bob")).unwrap(),
        ],
        4,
        1,
        2,
        2,
    );
    let server_uri = server.uri();

    let page1 = Page::from_page_response(page1_resp).with_fetcher(move |page_num: u64| {
        let uri = server_uri.clone();
        Box::pin(async move {
            let response = reqwest::Client::new()
                .post(format!("{}{}", uri, "/v3/workspaces/ws1/peers/list"))
                .query(&[("page", page_num)])
                .query(&[("size", 2u64)])
                .header("content-type", "application/json")
                .json(&serde_json::json!({}))
                .send()
                .await
                .map_err(HonchoError::Transport)?;
            let pr: PageResponse<Peer> = response.json().await.map_err(HonchoError::Transport)?;
            Ok(pr)
        })
    });

    let stream = page1.into_stream();
    let mut stream = Box::pin(stream);
    let first = stream.next().await;
    assert!(first.is_some());
    drop(stream);

    let requests = server.received_requests().await.unwrap();
    assert_eq!(
        requests.len(),
        0,
        "page 2 should not be fetched after dropping stream"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// P1.8 — Error propagation when page 2 returns HTTP 500
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn page_next_page_returns_err_on_http_500() {
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/list"))
        .and(query_param("page", "2"))
        .respond_with(
            ResponseTemplate::new(500).set_body_json(serde_json::json!({"error": "internal"})),
        )
        .mount(&server)
        .await;

    let page1_resp = PageResponse::<Peer>::new(
        vec![
            serde_json::from_value(peer_json("alice")).unwrap(),
            serde_json::from_value(peer_json("bob")).unwrap(),
        ],
        4,
        1,
        2,
        2,
    );
    let server_uri = server.uri();

    let page1 = Page::from_page_response(page1_resp).with_fetcher(move |page_num: u64| {
        let uri = server_uri.clone();
        Box::pin(async move {
            let response = reqwest::Client::new()
                .post(format!("{}{}", uri, "/v3/workspaces/ws1/peers/list"))
                .query(&[("page", page_num)])
                .query(&[("size", 2u64)])
                .header("content-type", "application/json")
                .json(&serde_json::json!({}))
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
    });

    assert!(page1.has_next());
    let result = page1.next_page().await;
    assert!(
        result.is_err(),
        "next_page should return Err on HTTP 500, not Ok(None)"
    );
}

#[tokio::test]
async fn page_collect_all_returns_err_on_http_500() {
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/list"))
        .and(query_param("page", "2"))
        .respond_with(
            ResponseTemplate::new(500).set_body_json(serde_json::json!({"error": "internal"})),
        )
        .mount(&server)
        .await;

    let page1_resp = PageResponse::<Peer>::new(
        vec![
            serde_json::from_value(peer_json("alice")).unwrap(),
            serde_json::from_value(peer_json("bob")).unwrap(),
        ],
        4,
        1,
        2,
        2,
    );
    let server_uri = server.uri();

    let page1 = Page::from_page_response(page1_resp).with_fetcher(move |page_num: u64| {
        let uri = server_uri.clone();
        Box::pin(async move {
            let response = reqwest::Client::new()
                .post(format!("{}{}", uri, "/v3/workspaces/ws1/peers/list"))
                .query(&[("page", page_num)])
                .query(&[("size", 2u64)])
                .header("content-type", "application/json")
                .json(&serde_json::json!({}))
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
    });

    let mut all = Vec::new();
    let mut current = page1;
    all.append(&mut current.items());
    let result: Result<Vec<Peer>, _> = {
        let mut collected = all;
        loop {
            match current.next_page().await {
                Ok(Some(next)) => {
                    let mut items = next.items();
                    collected.append(&mut items);
                    current = next;
                }
                Ok(None) => break Ok(collected),
                Err(e) => break Err(e),
            }
        }
    };

    assert!(
        result.is_err(),
        "collect-all loop should return Err on HTTP 500, not Ok(partial_vec)"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// F3.x — Page::map transforms items
// ═══════════════════════════════════════════════════════════════════════
#[tokio::test]
async fn page_map_transforms_items() {
    let peers = vec![
        serde_json::from_value::<Peer>(peer_json("alice")).unwrap(),
        serde_json::from_value::<Peer>(peer_json("bob")).unwrap(),
    ];
    let page = Page::new(peers, 2, 1, 50, 1);

    let mapped: Page<Peer, String> = page.map(|p| p.id);

    assert_eq!(mapped.items(), vec!["alice".to_string(), "bob".to_string()]);
    assert_eq!(mapped.total(), 2);
    assert_eq!(mapped.page(), 1);
    assert_eq!(mapped.raw_items().len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════
// PR5 — page-number overflow at u64::MAX must not panic (debug overflow checks)
//
// `has_next()` is defined as `page < pages`, so it can never be `true` when
// `page == u64::MAX`. The reachable hazard was the unconditional `page + 1`
// computed in `into_stream` (plus the defensive guard in `next_page`): both
// must now treat overflow as "no more pages" instead of panicking/wrapping.
// ═══════════════════════════════════════════════════════════════════════
#[tokio::test]
async fn page_max_page_number_does_not_overflow() {
    use futures_util::TryStreamExt;

    let page_resp = PageResponse::<Peer>::new(
        vec![serde_json::from_value(peer_json("alice")).unwrap()],
        1,
        u64::MAX,
        2,
        u64::MAX,
    );

    // Attach a fetcher that errors if ever invoked: at the u64::MAX boundary no
    // further page should be requested.
    let page: Page<Peer> = Page::from_page_response(page_resp).with_fetcher(|_page_num: u64| {
        Box::pin(async move {
            Err::<PageResponse<Peer>, HonchoError>(HonchoError::Validation(
                "fetcher must not be called past u64::MAX".into(),
            ))
        })
    });

    // `next_page()` must not panic and must report "no more pages".
    assert!(page.next_page().await.unwrap().is_none());

    // `into_stream()` must terminate without panicking and yield only the
    // current page's items (the fetcher is never invoked).
    let collected: Vec<Peer> = page.into_stream().try_collect().await.unwrap();
    assert_eq!(collected.len(), 1);
    assert_eq!(collected[0].id, "alice");
}
