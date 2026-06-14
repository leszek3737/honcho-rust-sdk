#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

mod common;
use common::{TEST_WORKSPACE_ID, make_honcho, mount_workspace_ensure, workspace_response};

/// Mounts `GET /v3/workspaces/ws1` returning the workspace body.
///
/// The lazy `ensure_workspace` path (driven via `get_configuration_raw`)
/// fetches the workspace after ensuring it exists, so every lazy-path test
/// needs this GET in place alongside the ensure POST.
async fn mount_workspace_get(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path(format!("/v3/workspaces/{TEST_WORKSPACE_ID}")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(workspace_response(TEST_WORKSPACE_ID)),
        )
        .mount(server)
        .await;
}

#[tokio::test]
async fn ensure_workspace_cached_on_single_instance() {
    // The lazy `ensure_workspace()` path (driven here by `get_configuration_raw`)
    // is single-flight AND caches success: repeated calls on one client issue the
    // ensure POST at most once.
    let server = MockServer::start().await;
    mount_workspace_ensure(&server, 1).await;
    mount_workspace_get(&server).await;

    let honcho = make_honcho(&server.uri());

    // Three sequential calls; success is cached after the first, so the
    // `.expect(1)` on the ensure POST still holds.
    honcho.get_configuration_raw().await.unwrap();
    honcho.get_configuration_raw().await.unwrap();
    honcho.get_configuration_raw().await.unwrap();

    server.verify().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ensure_workspace_concurrent_calls_only_one_request() {
    // Concurrent public calls race on the lazy ensure; the 50ms delay widens the
    // window. Single-flight must still collapse them to one ensure POST.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v3/workspaces"))
        .and(body_json(json!({ "id": TEST_WORKSPACE_ID })))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(workspace_response(TEST_WORKSPACE_ID))
                .set_delay(Duration::from_millis(50)),
        )
        .expect(1)
        .mount(&server)
        .await;
    mount_workspace_get(&server).await;

    let honcho = make_honcho(&server.uri());

    let handles: Vec<_> = (0..5)
        .map(|_| {
            let h = honcho.clone();
            tokio::spawn(async move { h.get_configuration_raw().await })
        })
        .collect();

    for handle in handles {
        handle.await.unwrap().unwrap();
    }

    server.verify().await;
}

#[tokio::test]
async fn ensure_workspace_409_conflict_is_success() {
    // A 409 Conflict means the workspace already exists. `ensure_workspace`
    // treats it as success (Ok(())), not an error.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v3/workspaces"))
        .and(body_json(json!({ "id": TEST_WORKSPACE_ID })))
        .respond_with(ResponseTemplate::new(409))
        .expect(1)
        .mount(&server)
        .await;

    let honcho = make_honcho(&server.uri());

    // `force_ensure` issues exactly one ensure POST and no follow-up GET, so a
    // 409 here exercises the "already exists = success" branch directly.
    honcho.force_ensure().await.unwrap();

    server.verify().await;
}

#[tokio::test]
async fn force_ensure_failure_retries_next_call() {
    // POST is non-idempotent, so it is never retried within a single call.
    // `force_ensure` bypasses the cache on every call: the first fails on 503,
    // the second re-issues and succeeds. Two POSTs total.
    let server = MockServer::start().await;
    let post_hits = Arc::new(AtomicU32::new(0));
    let hits = post_hits.clone();
    let body = workspace_response(TEST_WORKSPACE_ID);

    Mock::given(method("POST"))
        .and(path("/v3/workspaces"))
        .and(body_json(json!({ "id": TEST_WORKSPACE_ID })))
        .respond_with(move |_: &Request| {
            // First attempt fails; later attempts succeed.
            if post_hits.fetch_add(1, Ordering::SeqCst) == 0 {
                ResponseTemplate::new(503)
            } else {
                ResponseTemplate::new(200).set_body_json(&body)
            }
        })
        .expect(2)
        .mount(&server)
        .await;

    let honcho = make_honcho(&server.uri());

    let err = honcho.force_ensure().await.unwrap_err();
    assert_eq!(err.status_code(), Some(503));

    honcho.force_ensure().await.unwrap();

    // The Arc clone proves both attempts hit the server (2 POSTs).
    assert_eq!(hits.load(Ordering::SeqCst), 2);
    server.verify().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ensure_workspace_init_failure_not_cached_then_concurrent_retry() {
    // `get_or_try_init` does NOT cache errors: a failed ensure leaves the
    // OnceCell empty. The first lazy ensure fails (500); a later wave of
    // concurrent calls re-runs init, single-flights to ONE successful POST, and
    // every caller observes Ok. Two POSTs total (1 failed + 1 shared retry).
    let server = MockServer::start().await;
    let post_hits = Arc::new(AtomicU32::new(0));
    let hits = post_hits.clone();
    let body = workspace_response(TEST_WORKSPACE_ID);

    Mock::given(method("POST"))
        .and(path("/v3/workspaces"))
        .and(body_json(json!({ "id": TEST_WORKSPACE_ID })))
        .respond_with(move |_: &Request| {
            // First init attempt fails; the error is not cached, so the retry
            // wave succeeds.
            if post_hits.fetch_add(1, Ordering::SeqCst) == 0 {
                ResponseTemplate::new(500)
            } else {
                ResponseTemplate::new(200).set_body_json(&body)
            }
        })
        .expect(2)
        .mount(&server)
        .await;
    mount_workspace_get(&server).await;

    let honcho = make_honcho(&server.uri());

    // First lazy ensure fails; the error is not cached in the OnceCell.
    let first = honcho.get_configuration_raw().await;
    assert_eq!(first.unwrap_err().status_code(), Some(500));

    // Concurrent retry wave: single-flight collapses the retries to one POST,
    // which now returns 200 for every caller.
    let handles: Vec<_> = (0..5)
        .map(|_| {
            let h = honcho.clone();
            tokio::spawn(async move { h.get_configuration_raw().await })
        })
        .collect();
    for handle in handles {
        handle.await.unwrap().unwrap();
    }

    // 1 failed init + 1 shared successful retry = 2 POSTs.
    assert_eq!(hits.load(Ordering::SeqCst), 2);
    server.verify().await;
}
