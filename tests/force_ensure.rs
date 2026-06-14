//! Workspace-ensure behavior.
//!
//! Two distinct mechanisms are exercised here:
//!
//! * the lazy, single-flight `ensure_workspace` cache (a per-instance
//!   `tokio::sync::OnceCell` driven by `get_or_try_init`), which collapses
//!   concurrent first-time ensures into a single `POST /v3/workspaces`; and
//! * `force_ensure`, which deliberately *bypasses* that cache — it resets the
//!   `OnceCell` on every call so it always re-issues the create request (used
//!   to recover after a server-side workspace delete).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use common::{
    TEST_WORKSPACE_ID, make_honcho, mount_workspace_ensure, workspace_ensure_mock,
    workspace_response,
};
use honcho_ai::error::HonchoError;
use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

// ════════════════════════════════════════════════════════════════════════
// Single-flight under real concurrency (the lazy ensure cache)
// ════════════════════════════════════════════════════════════════════════

/// HEADLINE: a real single-flight concurrency test.
///
/// On ONE client, `N` tasks race on the first-time lazy ensure. The shared
/// `OnceCell` must collapse them into exactly one `POST /v3/workspaces`
/// (`.expect(1)`), and every task must observe `Ok`.
///
/// `force_ensure` resets the cache on each call, so it cannot exhibit
/// single-flight; the property is proven here through the lazy path that
/// `force_ensure` builds on, driven via the public `get_configuration_raw`.
/// `tokio::sync::OnceCell` serializes initialization internally, so the single
/// POST is guaranteed regardless of scheduling — the test is deterministic.
#[tokio::test(flavor = "multi_thread")]
async fn ensure_workspace_single_flight_collapses_concurrent_calls() {
    const N: usize = 8;

    let server = MockServer::start().await;

    // The ensure POST must fire exactly once despite N racing callers.
    mount_workspace_ensure(&server, 1).await;

    // `get_configuration_raw` GETs the workspace after ensuring it; this GET is
    // not single-flighted, so it has no call-count expectation.
    let config_path = format!("/v3/workspaces/{TEST_WORKSPACE_ID}");
    Mock::given(method("GET"))
        .and(path(config_path))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(workspace_response(TEST_WORKSPACE_ID)),
        )
        .mount(&server)
        .await;

    let honcho = make_honcho(&server.uri());

    let mut handles = Vec::with_capacity(N);
    for _ in 0..N {
        let h = honcho.clone();
        handles.push(tokio::spawn(async move { h.get_configuration_raw().await }));
    }
    for handle in handles {
        // Outer `unwrap`: task did not panic. Inner `unwrap`: request was `Ok`.
        handle.await.unwrap().unwrap();
    }

    // wiremock verifies `.expect(1)`: exactly one ensure POST was sent.
    server.verify().await;
}

/// `force_ensure` under real parallelism: every call resets the cache, so `N`
/// concurrent calls all succeed and issue between `1` and `N` create requests.
///
/// The exact count is racy *by design*: `force_ensure` resets the `OnceCell`
/// then snapshots it under separate short locks, so an interleaved reset may let
/// later callers share a freshly-reset cell (collapsing to one POST) or not
/// (issuing their own). We assert the deterministic safe envelope instead of an
/// exact count: no panics/data races, all `Ok`, and `1..=N` POSTs.
#[tokio::test(flavor = "multi_thread")]
async fn force_ensure_concurrent_calls_all_succeed() {
    const N: u32 = 8;

    let server = MockServer::start().await;
    let hits = Arc::new(AtomicU32::new(0));
    let counter = Arc::clone(&hits);
    let body = workspace_response(TEST_WORKSPACE_ID);

    Mock::given(method("POST"))
        .and(path("/v3/workspaces"))
        .and(body_json(json!({ "id": TEST_WORKSPACE_ID })))
        .respond_with(move |_: &Request| {
            // Plain hit counter: `Relaxed` is enough, we only read it after all
            // tasks have joined (a happens-before established by the joins).
            counter.fetch_add(1, Ordering::Relaxed);
            ResponseTemplate::new(200).set_body_json(&body)
        })
        .mount(&server)
        .await;

    let honcho = make_honcho(&server.uri());

    let mut handles = Vec::with_capacity(N as usize);
    for _ in 0..N {
        let h = honcho.clone();
        handles.push(tokio::spawn(async move { h.force_ensure().await }));
    }
    for handle in handles {
        handle.await.unwrap().unwrap();
    }

    let count = hits.load(Ordering::Relaxed);
    assert!(
        (1..=N).contains(&count),
        "force_ensure issued {count} create requests, expected 1..={N}"
    );
}

// ════════════════════════════════════════════════════════════════════════
// force_ensure error semantics
// ════════════════════════════════════════════════════════════════════════

/// Errors are never cached: a failed ensure must not poison anything, so a later
/// `force_ensure` retries and succeeds. First call 500 => `Err`; second call
/// 200 => `Ok`. POST is non-idempotent (not retried within a call), so two
/// calls => exactly two POSTs (`.expect(2)`).
#[tokio::test]
async fn force_ensure_retries_after_uncached_error() {
    let server = MockServer::start().await;
    let calls = Arc::new(AtomicU32::new(0));
    let counter = Arc::clone(&calls);
    let body = workspace_response(TEST_WORKSPACE_ID);

    Mock::given(method("POST"))
        .and(path("/v3/workspaces"))
        .and(body_json(json!({ "id": TEST_WORKSPACE_ID })))
        .respond_with(move |_: &Request| {
            let n = counter.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                ResponseTemplate::new(500)
            } else {
                ResponseTemplate::new(200).set_body_json(&body)
            }
        })
        .expect(2)
        .mount(&server)
        .await;

    let honcho = make_honcho(&server.uri());

    let err = honcho
        .force_ensure()
        .await
        .expect_err("first ensure should surface the 500");
    assert!(
        matches!(err, HonchoError::Server { status: 500, .. }),
        "first ensure should surface the 500, got {err:?}"
    );
    honcho.force_ensure().await.unwrap();

    server.verify().await;
}

/// A 409 Conflict means the workspace already exists; `ensure_workspace` maps it
/// to `Ok(())`. `force_ensure` re-issues on every call, so two calls => two
/// 409s, both `Ok`.
#[tokio::test]
async fn force_ensure_conflict_409_is_ok() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces"))
        .and(body_json(json!({ "id": TEST_WORKSPACE_ID })))
        .respond_with(
            ResponseTemplate::new(409).set_body_json(json!({ "detail": "workspace exists" })),
        )
        .expect(2)
        .mount(&server)
        .await;

    let honcho = make_honcho(&server.uri());

    honcho.force_ensure().await.unwrap();
    honcho.force_ensure().await.unwrap();

    server.verify().await;
}

/// A non-409 API error propagates instead of being swallowed: a 500 surfaces as
/// `HonchoError::Server { status: 500, .. }`. POST is not retried, so the single
/// failing call sends exactly one POST.
#[tokio::test]
async fn force_ensure_server_error_500_is_err() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces"))
        .and(body_json(json!({ "id": TEST_WORKSPACE_ID })))
        .respond_with(ResponseTemplate::new(500))
        .expect(1)
        .mount(&server)
        .await;

    let honcho = make_honcho(&server.uri());

    let err = honcho.force_ensure().await.unwrap_err();
    assert!(
        matches!(err, HonchoError::Server { status: 500, .. }),
        "expected HonchoError::Server {{ status: 500 }}, got {err:?}"
    );
    assert_eq!(err.status_code(), Some(500));

    server.verify().await;
}

// ════════════════════════════════════════════════════════════════════════
// force_ensure cache-bypass semantics
// ════════════════════════════════════════════════════════════════════════

/// `force_ensure` bypasses the cache on every call, so `N` sequential calls on
/// one instance issue `N` create requests — this is how it recovers after a
/// server-side workspace delete.
#[tokio::test]
async fn force_ensure_reissues_on_every_call() {
    let server = MockServer::start().await;
    workspace_ensure_mock(TEST_WORKSPACE_ID)
        .expect(3)
        .mount(&server)
        .await;

    let honcho = make_honcho(&server.uri());
    for _ in 0..3 {
        honcho.force_ensure().await.unwrap();
    }

    server.verify().await;
}

/// BUG FIX (was `force_ensure_idempotent_same_workspace_id`, which used two
/// instances yet claimed idempotence): the ensure cache is *per instance*, so
/// two clients sharing a workspace id do NOT deduplicate — each issues its own
/// create request. Two instances => two POSTs.
#[tokio::test]
async fn force_ensure_separate_instances_each_issue_request() {
    let server = MockServer::start().await;
    workspace_ensure_mock(TEST_WORKSPACE_ID)
        .expect(2)
        .mount(&server)
        .await;

    let a = make_honcho(&server.uri());
    let b = make_honcho(&server.uri());

    a.force_ensure().await.unwrap();
    b.force_ensure().await.unwrap();

    server.verify().await;
}

// ════════════════════════════════════════════════════════════════════════
// Blocking API (mirrors the async force_ensure contract)
// ════════════════════════════════════════════════════════════════════════

/// Current-thread runtime used to drive the async wiremock fixtures from the
/// synchronous blocking tests. The blocking client owns its own runtime, so its
/// `force_ensure` is always called *outside* `block_on`.
#[cfg(feature = "blocking")]
fn blocking_rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build current-thread runtime")
}

#[cfg(feature = "blocking")]
fn blocking_honcho(base_url: &str, workspace_id: &str) -> honcho_ai::blocking::Honcho {
    honcho_ai::blocking::Honcho::from_params(
        honcho_ai::Honcho::builder()
            .base_url(base_url)
            .workspace_id(workspace_id)
            .build(),
    )
    .expect("construct blocking Honcho test client")
}

/// Blocking `force_ensure` mirrors the async contract: every call bypasses the
/// cache and re-issues `POST /v3/workspaces`. Two calls => two POSTs.
#[cfg(feature = "blocking")]
#[test]
fn blocking_force_ensure_reissues_every_call() {
    let rt = blocking_rt();
    let server = rt.block_on(MockServer::start());
    rt.block_on(workspace_ensure_mock("ws-blk").expect(2).mount(&server));

    let honcho = blocking_honcho(&server.uri(), "ws-blk");
    honcho.force_ensure().unwrap();
    honcho.force_ensure().unwrap();

    rt.block_on(server.verify());
}

/// Blocking, per-instance cache: two clients sharing a workspace id each issue
/// their own create request. Two instances => two POSTs.
#[cfg(feature = "blocking")]
#[test]
fn blocking_force_ensure_separate_instances_each_issue_request() {
    let rt = blocking_rt();
    let server = rt.block_on(MockServer::start());
    rt.block_on(workspace_ensure_mock("ws-blk2").expect(2).mount(&server));

    let a = blocking_honcho(&server.uri(), "ws-blk2");
    let b = blocking_honcho(&server.uri(), "ws-blk2");
    a.force_ensure().unwrap();
    b.force_ensure().unwrap();

    rt.block_on(server.verify());
}
