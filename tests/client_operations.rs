#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::needless_borrows_for_generic_args,
    clippy::unused_async,
    missing_docs
)]

mod common;

use std::collections::HashMap;

use common::make_honcho_no_retry;
use honcho_ai::client::Honcho;
use honcho_ai::error::HonchoError;
use honcho_ai::types::dream::QueueStatus;
use serde_json::{Value, json};
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn workspace_json() -> serde_json::Value {
    json!({
        "id": "ws1",
        "metadata": {},
        "configuration": {},
        "created_at": "2025-01-15T10:30:00Z"
    })
}

fn message_json(id: &str) -> serde_json::Value {
    json!({
        "id": id,
        "content": "hello world",
        "peer_id": "alice",
        "session_id": "sess1",
        "created_at": "2025-01-15T10:30:00Z",
        "metadata": {},
        "workspace_id": "ws1",
        "token_count": 2
    })
}

async fn mount_ensure_workspace(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/v3/workspaces"))
        .and(body_json(json!({"id": "ws1"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(workspace_json()))
        // The workspace-ensure POST fires exactly once per client setup; a
        // double-fetch or a skipped fetch is caught on server drop.
        .expect(1)
        .mount(server)
        .await;
}

/// Starts a fresh per-test mock server and a Honcho client pointed at it.
///
/// Each test gets its own server (no shared global state, no cross-test races).
/// Callers mount the workspace-ensure POST and the endpoint under test on top.
async fn setup() -> (MockServer, Honcho) {
    let server = MockServer::start().await;
    // No-retry client so the error-path tests below fail fast and
    // deterministically: a 500 on an idempotent verb is otherwise retried (with
    // backoff sleeps) before surfacing the error.
    let honcho = make_honcho_no_retry(&server.uri());
    (server, honcho)
}

#[tokio::test]
async fn search_returns_messages() {
    let (server, honcho) = setup().await;
    mount_ensure_workspace(&server).await;

    let filters: HashMap<String, Value> = HashMap::from([("peer_id".to_owned(), json!("alice"))]);

    // The request body must carry the query plus the explicit limit and filters,
    // proving the builder forwards both onto the wire (not just the query).
    let expected_body = json!({
        "query": "hello",
        "limit": 5,
        "filters": { "peer_id": "alice" }
    });

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/search"))
        .and(body_json(expected_body))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!([message_json("m1"), message_json("m2")])),
        )
        .expect(1)
        .mount(&server)
        .await;

    let results = honcho
        .search("hello")
        .limit(5)
        .filters(filters)
        .build()
        .await
        .unwrap();

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].id(), "m1");
    assert_eq!(results[1].id(), "m2");

    // Attribution / payload fields must survive deserialization, not just the id.
    let first = &results[0];
    assert_eq!(first.content(), "hello world");
    assert_eq!(first.peer_id(), "alice");
    assert_eq!(first.session_id(), "sess1");
    assert_eq!(first.token_count(), 2);
    assert!(first.metadata().is_empty());
}

#[tokio::test]
async fn search_maps_server_error() {
    let (server, honcho) = setup().await;
    mount_ensure_workspace(&server).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/search"))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({ "detail": "boom" })))
        // POST is non-idempotent and retries are disabled, so the 500 surfaces
        // after exactly one attempt.
        .expect(1)
        .mount(&server)
        .await;

    let err = honcho.search("hello").build().await.unwrap_err();

    // The consolidated API has no single `Api` variant: a 5xx maps to `Server`,
    // and `status_code()` exposes the original code regardless of variant.
    assert!(
        matches!(err, HonchoError::Server { status: 500, .. }),
        "expected Server(500), got {err:?}"
    );
    assert_eq!(err.status_code(), Some(500));
}

#[tokio::test]
async fn queue_status_returns_status() {
    let (server, honcho) = setup().await;
    mount_ensure_workspace(&server).await;

    let response_body = json!({
        "total_work_units": 10,
        "completed_work_units": 8,
        "in_progress_work_units": 1,
        "pending_work_units": 1,
        "sessions": {
            "sess1": {
                "session_id": "sess1",
                "total_work_units": 5,
                "completed_work_units": 4,
                "in_progress_work_units": 1,
                "pending_work_units": 0
            }
        }
    });

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/queue/status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response_body))
        .expect(1)
        .mount(&server)
        .await;

    let status: QueueStatus = honcho.queue_status(None, None, None).await.unwrap();
    assert_eq!(status.total_work_units, 10);
    assert_eq!(status.completed_work_units, 8);
    assert_eq!(status.in_progress_work_units, 1);
    assert_eq!(status.pending_work_units, 1);

    // Assert the nested per-session counts, not just that the map is present.
    let sessions = status.sessions.expect("sessions present in response");
    assert_eq!(sessions["sess1"].completed_work_units, 4);
}

#[tokio::test]
async fn queue_status_passes_filters_as_query_params() {
    let (server, honcho) = setup().await;
    mount_ensure_workspace(&server).await;

    let response_body = json!({
        "total_work_units": 3,
        "completed_work_units": 1,
        "in_progress_work_units": 1,
        "pending_work_units": 1,
        "sessions": {
            "sess1": {
                "session_id": "sess1",
                "total_work_units": 3,
                "completed_work_units": 1,
                "in_progress_work_units": 1,
                "pending_work_units": 1
            }
        }
    });

    // The three optional filters must reach the wire as query params; a missing
    // or renamed param means no mock matches and the call fails instead.
    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/queue/status"))
        .and(query_param("observer_id", "obs1"))
        .and(query_param("sender_id", "send1"))
        .and(query_param("session_id", "sess1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response_body))
        .expect(1)
        .mount(&server)
        .await;

    let status = honcho
        .queue_status(Some("obs1"), Some("send1"), Some("sess1"))
        .await
        .unwrap();

    // The `Some(sessions)` branch: the map is populated and decoded.
    let sessions = status.sessions.expect("sessions present in response");
    assert_eq!(sessions["sess1"].completed_work_units, 1);
}

#[tokio::test]
async fn queue_status_maps_not_found() {
    let (server, honcho) = setup().await;
    mount_ensure_workspace(&server).await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/queue/status"))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(json!({ "detail": "queue not found" })),
        )
        // 404 is non-retryable, so a single GET surfaces the NotFound.
        .expect(1)
        .mount(&server)
        .await;

    let err = honcho.queue_status(None, None, None).await.unwrap_err();

    // 404 maps to the dedicated `NotFound` variant (no generic `Api`).
    assert!(
        matches!(err, HonchoError::NotFound { .. }),
        "expected NotFound, got {err:?}"
    );
    assert_eq!(err.status_code(), Some(404));
}

#[tokio::test]
async fn schedule_dream_posts_correct_body() {
    let (server, honcho) = setup().await;
    mount_ensure_workspace(&server).await;

    // observed defaults to observer; session_id is omitted from the body entirely.
    let expected_body = json!({
        "observer": "alice",
        "observed": "alice",
        "dream_type": "omni"
    });

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/schedule_dream"))
        .and(body_json(expected_body))
        // schedule_dream returns 204 (empty body) per the OpenAPI spec; the SDK
        // decodes any empty-body 2xx into `()`. delete_workspace below uses 202
        // for the same reason — both rely on the identical empty-body handling.
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    honcho.schedule_dream("alice", None, None).await.unwrap();
}

#[tokio::test]
async fn schedule_dream_with_session_and_observed() {
    let (server, honcho) = setup().await;
    mount_ensure_workspace(&server).await;

    // Explicit observed peer (distinct from observer) plus a session scope: both
    // optionals must appear in the body.
    let expected_body = json!({
        "observer": "alice",
        "observed": "bob",
        "dream_type": "omni",
        "session_id": "sess1"
    });

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/schedule_dream"))
        .and(body_json(expected_body))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    honcho
        .schedule_dream("alice", Some("sess1"), Some("bob"))
        .await
        .unwrap();
}

#[tokio::test]
async fn schedule_dream_rejects_empty_observer_before_request() {
    let (server, honcho) = setup().await;

    let err = honcho.schedule_dream("", None, None).await.unwrap_err();
    assert!(matches!(
        err,
        HonchoError::Validation(ref message) if message == "observer must not be empty"
    ));

    let requests = server.received_requests().await.unwrap();
    assert!(requests.is_empty(), "no request should be sent");
}

#[tokio::test]
async fn delete_workspace_calls_delete() {
    let (server, honcho) = setup().await;

    Mock::given(method("DELETE"))
        .and(path("/v3/workspaces/ws_to_delete"))
        // delete returns 202 Accepted (empty body) per the OpenAPI spec; same
        // empty-body 2xx -> `()` handling as schedule_dream's 204 above.
        .respond_with(ResponseTemplate::new(202))
        .expect(1)
        .mount(&server)
        .await;

    honcho.delete_workspace("ws_to_delete").await.unwrap();

    // Prove the DELETE actually went on the wire (verb + path), not just that the
    // call returned Ok.
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method.as_str(), "DELETE");
    assert_eq!(requests[0].url.path(), "/v3/workspaces/ws_to_delete");
}
