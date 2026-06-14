#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::needless_pass_by_value,
    missing_docs
)]

//! Workspace-level metadata & configuration: typed and raw get/set, the
//! retry/backoff behaviour on transient 5xx, and constructor validation.

mod common;

use std::collections::HashMap;

use common::{TEST_WORKSPACE_ID, make_honcho, make_honcho_no_retry, mount_workspace_ensure};
use honcho_ai::Honcho;
use honcho_ai::error::HonchoError;
use honcho_ai::types::common::ReasoningConfiguration;
use honcho_ai::types::workspace::WorkspaceConfiguration;
use serde_json::{Value, json};
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Path of the workspace resource for the shared test workspace.
fn ws_path() -> String {
    format!("/v3/workspaces/{TEST_WORKSPACE_ID}")
}

/// A full `Workspace` JSON body with caller-chosen `metadata` / `configuration`.
///
/// The shared [`common::workspace_response`] hard-codes empty objects, which the
/// get-tests below cannot use because they assert on specific contents.
fn workspace_body(metadata: Value, configuration: Value) -> Value {
    json!({
        "id": TEST_WORKSPACE_ID,
        "metadata": metadata,
        "configuration": configuration,
        "created_at": "2025-01-15T10:30:00Z"
    })
}

// ── metadata: get ────────────────────────────────────────────────────────

#[tokio::test]
async fn gets_workspace_metadata_by_get() {
    let server = MockServer::start().await;
    mount_workspace_ensure(&server, 1).await;

    let body = workspace_body(json!({"env": "production", "team": "core"}), json!({}));
    Mock::given(method("GET"))
        .and(path(ws_path()))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(&server)
        .await;

    let result = make_honcho(&server.uri()).get_metadata().await.unwrap();

    assert_eq!(result.get("env"), Some(&json!("production")));
    assert_eq!(result.get("team"), Some(&json!("core")));
}

#[tokio::test]
async fn get_metadata_empty_when_no_metadata() {
    let server = MockServer::start().await;
    mount_workspace_ensure(&server, 1).await;

    Mock::given(method("GET"))
        .and(path(ws_path()))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(workspace_body(json!({}), json!({}))),
        )
        .expect(1)
        .mount(&server)
        .await;

    let result = make_honcho(&server.uri()).get_metadata().await.unwrap();

    assert!(result.is_empty());
}

#[tokio::test]
async fn get_metadata_surfaces_decode_error_on_malformed_json() {
    let server = MockServer::start().await;
    mount_workspace_ensure(&server, 1).await;

    // 200 OK but a body that is not valid JSON must surface as a decode error,
    // never silently as an empty map.
    Mock::given(method("GET"))
        .and(path(ws_path()))
        .respond_with(ResponseTemplate::new(200).set_body_string("{ not valid json"))
        .expect(1)
        .mount(&server)
        .await;

    let err = make_honcho(&server.uri()).get_metadata().await.unwrap_err();

    assert!(
        matches!(err, HonchoError::Decode { .. }),
        "expected Decode, got {err:?}"
    );
}

// ── metadata: set ────────────────────────────────────────────────────────

#[tokio::test]
async fn set_metadata_puts_to_workspace_id() {
    let server = MockServer::start().await;

    let metadata = json!({"env": "staging"});
    // `set_metadata` is a pure write: it does NOT lazily ensure the workspace.
    Mock::given(method("PUT"))
        .and(path(ws_path()))
        .and(body_json(json!({"metadata": metadata})))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(workspace_body(metadata.clone(), json!({}))),
        )
        .expect(1)
        .mount(&server)
        .await;

    make_honcho(&server.uri())
        .set_metadata(HashMap::from([("env".to_string(), json!("staging"))]))
        .await
        .unwrap();
}

#[tokio::test]
async fn set_metadata_server_error_returns_error() {
    let server = MockServer::start().await;

    // Disable retries so a single 503 is observed exactly once and the test
    // does not sleep through any backoff.
    let honcho = make_honcho_no_retry(&server.uri());

    Mock::given(method("PUT"))
        .and(path(ws_path()))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&server)
        .await;

    let err = honcho
        .set_metadata(HashMap::from([("env".to_string(), json!("staging"))]))
        .await
        .unwrap_err();

    assert!(
        matches!(err, HonchoError::Server { status: 503, .. }),
        "expected Server(503), got {err:?}"
    );
}

#[tokio::test]
async fn set_metadata_retries_503_until_exhausted() {
    let server = MockServer::start().await;

    // PUT is idempotent, so a 503 is retried. The default policy is 2 retries,
    // i.e. 1 initial attempt + 2 retries = 3 requests. `retry-after: 0` pins the
    // backoff to zero, so the call-count assertion is deterministic and the test
    // does not sleep (a paused clock can't be used here: it would fire reqwest's
    // request timeout before the real wiremock socket responds).
    Mock::given(method("PUT"))
        .and(path(ws_path()))
        .respond_with(ResponseTemplate::new(503).insert_header("retry-after", "0"))
        .expect(3)
        .mount(&server)
        .await;

    let err = make_honcho(&server.uri())
        .set_metadata(HashMap::from([("env".to_string(), json!("staging"))]))
        .await
        .unwrap_err();

    assert!(
        matches!(err, HonchoError::Server { status: 503, .. }),
        "expected Server(503), got {err:?}"
    );
}

// ── configuration: typed get/set ─────────────────────────────────────────

#[tokio::test]
async fn gets_workspace_configuration_by_get() {
    let server = MockServer::start().await;
    mount_workspace_ensure(&server, 1).await;

    let body = workspace_body(json!({}), json!({"reasoning": {"enabled": true}}));
    Mock::given(method("GET"))
        .and(path(ws_path()))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(&server)
        .await;

    let result = make_honcho(&server.uri())
        .get_configuration()
        .await
        .unwrap();

    assert_eq!(result.reasoning.as_ref().unwrap().enabled, Some(true));
}

#[tokio::test]
async fn get_configuration_empty_when_no_configuration() {
    let server = MockServer::start().await;
    mount_workspace_ensure(&server, 1).await;

    Mock::given(method("GET"))
        .and(path(ws_path()))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(workspace_body(json!({}), json!({}))),
        )
        .expect(1)
        .mount(&server)
        .await;

    let result = make_honcho(&server.uri())
        .get_configuration()
        .await
        .unwrap();

    assert!(result.reasoning.is_none());
    assert!(result.peer_card.is_none());
    assert!(result.summary.is_none());
    assert!(result.dream.is_none());
}

#[tokio::test]
async fn set_configuration_puts_to_workspace_id() {
    let server = MockServer::start().await;

    let wire = json!({"reasoning": {"enabled": false}});
    Mock::given(method("PUT"))
        .and(path(ws_path()))
        .and(body_json(json!({"configuration": wire})))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(workspace_body(json!({}), wire.clone())),
        )
        .expect(1)
        .mount(&server)
        .await;

    // Build the typed config directly; both types are `#[non_exhaustive]`, so
    // start from `Default` and assign fields.
    let mut reasoning = ReasoningConfiguration::default();
    reasoning.enabled = Some(false);
    let mut config = WorkspaceConfiguration::default();
    config.reasoning = Some(reasoning);

    make_honcho(&server.uri())
        .set_configuration(&config)
        .await
        .unwrap();
}

// ── configuration: raw get/set ───────────────────────────────────────────

#[tokio::test]
async fn gets_workspace_configuration_raw_by_get() {
    let server = MockServer::start().await;
    mount_workspace_ensure(&server, 1).await;

    let body = workspace_body(
        json!({}),
        json!({"unknown_future_field": {"enabled": true}}),
    );
    Mock::given(method("GET"))
        .and(path(ws_path()))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(&server)
        .await;

    let result = make_honcho(&server.uri())
        .get_configuration_raw()
        .await
        .unwrap();

    assert_eq!(
        result.get("unknown_future_field"),
        Some(&json!({"enabled": true}))
    );
}

#[tokio::test]
async fn get_configuration_raw_falls_back_to_empty_map() {
    // The raw getter must degrade to an empty map (never panic) when the
    // `configuration` field is null, absent, or the whole body is not an object.
    let cases: [(&str, Value); 3] = [
        ("null configuration", workspace_body(json!({}), Value::Null)),
        (
            "missing configuration key",
            json!({"id": TEST_WORKSPACE_ID, "metadata": {}, "created_at": "2025-01-15T10:30:00Z"}),
        ),
        ("non-object body", json!([1, 2, 3])),
    ];

    for (label, body) in cases {
        let server = MockServer::start().await;
        mount_workspace_ensure(&server, 1).await;

        Mock::given(method("GET"))
            .and(path(ws_path()))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .expect(1)
            .mount(&server)
            .await;

        let result = make_honcho(&server.uri())
            .get_configuration_raw()
            .await
            .unwrap();

        assert!(result.is_empty(), "case {label:?}: expected empty map");
    }
}

#[tokio::test]
async fn set_configuration_raw_puts_to_workspace_id() {
    let server = MockServer::start().await;

    let wire = json!({"unknown_future_field": {"enabled": true}});
    Mock::given(method("PUT"))
        .and(path(ws_path()))
        .and(body_json(json!({"configuration": wire})))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(workspace_body(json!({}), wire.clone())),
        )
        .expect(1)
        .mount(&server)
        .await;

    let config = HashMap::from([("unknown_future_field".to_string(), json!({"enabled": true}))]);

    make_honcho(&server.uri())
        .set_configuration_raw(config)
        .await
        .unwrap();
}

// ── get failures (table-driven across the read operations) ───────────────

#[tokio::test]
async fn get_operations_return_error_when_get_fails() {
    // `get_metadata`, `get_configuration`, and `get_configuration_raw` share the
    // ensure-then-GET shape, so a 404 on the GET must surface identically.
    for op in ["metadata", "configuration", "configuration_raw"] {
        let server = MockServer::start().await;
        mount_workspace_ensure(&server, 1).await;

        Mock::given(method("GET"))
            .and(path(ws_path()))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({"error": "not found"})))
            .expect(1)
            .mount(&server)
            .await;

        let honcho = make_honcho(&server.uri());
        // `.err()` erases the differing `Ok` types into a common `Option<_>`.
        let err = match op {
            "metadata" => honcho.get_metadata().await.err(),
            "configuration" => honcho.get_configuration().await.err(),
            _ => honcho.get_configuration_raw().await.err(),
        }
        .unwrap();

        assert!(
            matches!(err, HonchoError::NotFound { .. }),
            "op {op}: expected NotFound, got {err:?}"
        );
    }
}

// ── constructor validation (synchronous, no server) ──────────────────────

#[test]
fn workspace_id_accessor() {
    let honcho = Honcho::new("http://localhost:8000", "my-workspace").unwrap();
    assert_eq!(honcho.workspace_id(), "my-workspace");
}

#[test]
fn honcho_constructor_rejects_invalid_url() {
    assert!(Honcho::new("not a url", "test-ws").is_err());
}

#[test]
fn honcho_constructor_rejects_invalid_base_urls() {
    for base_url in ["localhost:8000", "ftp://example.com", "http://"] {
        assert!(
            Honcho::new(base_url, "test-ws").is_err(),
            "accepted base_url: {base_url}"
        );
    }
}

#[test]
fn honcho_constructor_normalizes_subpath_trailing_slash() {
    let honcho = Honcho::new("http://localhost:8000/api/", "test-ws").unwrap();
    assert_eq!(honcho.base_url().as_str(), "http://localhost:8000/api");
}

#[test]
fn honcho_constructor_rejects_invalid_workspace_ids() {
    for workspace_id in ["", "has space", "slash/id", "nonascii-é"] {
        assert!(
            Honcho::new("http://localhost:8000", workspace_id).is_err(),
            "accepted workspace_id: {workspace_id}"
        );
    }
}

#[test]
fn honcho_constructor_workspace_id_length_boundary() {
    // 512 is the inclusive maximum; 513 must be rejected.
    assert!(
        Honcho::new("http://localhost:8000", &"a".repeat(512)).is_ok(),
        "512-char workspace_id should be accepted"
    );
    assert!(
        Honcho::new("http://localhost:8000", &"a".repeat(513)).is_err(),
        "513-char workspace_id should be rejected"
    );
}

#[test]
fn honcho_constructor_accepts_valid_workspace_id() {
    let honcho = Honcho::new("http://localhost:8000", "abc-XYZ_123").unwrap();
    assert_eq!(honcho.workspace_id(), "abc-XYZ_123");
}
