#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use honcho_ai::Honcho;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn workspace_json() -> serde_json::Value {
    serde_json::json!({
        "id": "ws1",
        "metadata": {},
        "configuration": {},
        "created_at": "2025-01-15T10:30:00Z"
    })
}

fn honcho(server: &MockServer, workspace_id: &str) -> Honcho {
    Honcho::from_params(
        Honcho::builder()
            .base_url(server.uri())
            .workspace_id(workspace_id)
            .build(),
    )
    .unwrap()
}

#[tokio::test]
async fn ensure_workspace_called_once_per_instance() {
    let server = MockServer::start().await;
    let create_body = serde_json::json!({"id": "ws1"});

    // The lazy `ensure_workspace()` path (driven here by `get_configuration_raw`)
    // is single-flight: the ensure POST is issued at most once per client.
    Mock::given(method("POST"))
        .and(path("/v3/workspaces"))
        .and(body_json(&create_body))
        .respond_with(ResponseTemplate::new(200).set_body_json(workspace_json()))
        .expect(1)
        .mount(&server)
        .await;

    // `get_configuration_raw` fetches the workspace after ensuring it.
    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(workspace_json()))
        .mount(&server)
        .await;

    let honcho = honcho(&server, "ws1");

    honcho.get_configuration_raw().await.unwrap();
    honcho.get_configuration_raw().await.unwrap();

    server.verify().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ensure_workspace_concurrent_calls_only_one_request() {
    let server = MockServer::start().await;
    let create_body = serde_json::json!({"id": "ws1"});

    // Concurrent public calls race on the lazy ensure; the 50ms delay widens the
    // window. Single-flight must still collapse them to one ensure POST.
    Mock::given(method("POST"))
        .and(path("/v3/workspaces"))
        .and(body_json(&create_body))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(workspace_json())
                .set_delay(Duration::from_millis(50)),
        )
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(workspace_json()))
        .mount(&server)
        .await;

    let honcho = honcho(&server, "ws1");

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
async fn ensure_workspace_failure_retries_next_call() {
    let server = MockServer::start().await;
    let call_count = Arc::new(AtomicU32::new(0));
    let ws_json = workspace_json();

    Mock::given(method("POST"))
        .respond_with(move |_: &Request| {
            // POST is non-idempotent, so it is not retried within a single call.
            // The first call fails on 503; the second call succeeds.
            let n = call_count.fetch_add(1, Ordering::SeqCst);
            if n < 1 {
                ResponseTemplate::new(503)
            } else {
                ResponseTemplate::new(200).set_body_json(&ws_json)
            }
        })
        .mount(&server)
        .await;

    let honcho = honcho(&server, "ws1");

    let result = honcho.force_ensure().await;
    assert!(
        result.is_err(),
        "expected error from first ensure_workspace"
    );

    honcho.force_ensure().await.unwrap();
}
