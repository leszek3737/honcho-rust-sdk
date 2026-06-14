//! Construction-time behaviour of the `Honcho` client builder.
//!
//! Covers the resolution precedence (explicit arg > env var > default),
//! base-URL normalization, workspace-id validation, and that builder fields
//! (`api_key`, `default_headers`, `default_query`, `timeout`, `max_retries`,
//! `http_client`) actually reach the wire.
//!
//! Env-var isolation: every test that reads a `HONCHO_*` variable is wrapped
//! with [`temp_env`] (which pins *all* relevant vars to a known value and
//! restores them afterwards, so the ambient shell/CI env cannot leak into the
//! assertion) and additionally marked `#[serial_test::serial]` so no two
//! env-mutating tests run concurrently. `temp_env` is used rather than raw
//! `std::env::set_var`/`remove_var` because those are `unsafe` under edition
//! 2024 and this crate sets `unsafe_code = "forbid"` package-wide.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::Duration;

use honcho_ai::client::{Environment, Honcho, HonchoParams};
use honcho_ai::error::HonchoError;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use common::http_client;

/// Build a client from params, failing loud on a setup bug.
fn build_ok(params: HonchoParams) -> Honcho {
    Honcho::from_params(params).expect("client should build")
}

// ── workspace_id resolution & validation ────────────────────────────────

#[test]
fn workspace_id_explicit_is_used() {
    // An explicit arg always wins over the env var, so this is env-independent.
    let client = build_ok(
        Honcho::builder()
            .base_url("http://localhost:8000")
            .workspace_id("my-workspace")
            .build(),
    );
    assert_eq!(client.workspace_id(), "my-workspace");
}

#[test]
fn accepts_valid_workspace_id_charset() {
    let client = build_ok(
        Honcho::builder()
            .base_url("http://localhost:8000")
            .workspace_id("abc-XYZ_123")
            .build(),
    );
    assert_eq!(client.workspace_id(), "abc-XYZ_123");
}

#[test]
fn rejects_invalid_workspace_ids() {
    for workspace_id in ["", "has space", "slash/id", "nonascii-é"] {
        let result = Honcho::from_params(
            Honcho::builder()
                .base_url("http://localhost:8000")
                .workspace_id(workspace_id)
                .build(),
        );
        assert!(
            result.is_err(),
            "workspace_id {workspace_id:?} should be rejected"
        );
        let err = result.err().expect("checked is_err above");
        assert!(
            matches!(err, HonchoError::Configuration(_)),
            "workspace_id {workspace_id:?}: expected Configuration, got {err:?}"
        );
    }
}

#[test]
fn accepts_max_length_workspace_id() {
    // 512 is the inclusive upper bound (`len() > 512` is the rejection rule).
    let id = "a".repeat(512);
    let client = build_ok(
        Honcho::builder()
            .base_url("http://localhost:8000")
            .workspace_id(id.clone())
            .build(),
    );
    assert_eq!(client.workspace_id(), id);
}

#[test]
fn rejects_too_long_workspace_id() {
    // 513 is the first rejected length.
    let result = Honcho::from_params(
        Honcho::builder()
            .base_url("http://localhost:8000")
            .workspace_id("a".repeat(513))
            .build(),
    );
    let err = result
        .err()
        .expect("513-char workspace_id must be rejected");
    assert!(
        matches!(err, HonchoError::Configuration(_)),
        "expected Configuration, got {err:?}"
    );
}

// ── base_url normalization & validation ─────────────────────────────────

#[test]
fn rejects_invalid_base_urls() {
    // Empty arg is *not* treated as "unset" (no `.filter(is_empty)` on the
    // explicit arg), so it must fail rather than fall back to the default.
    for base_url in ["", "localhost:8000", "ftp://example.com", "http://"] {
        let result = Honcho::from_params(Honcho::builder().base_url(base_url).build());
        assert!(result.is_err(), "base_url {base_url:?} should be rejected");
        let err = result.err().expect("checked is_err above");
        assert!(
            matches!(err, HonchoError::Configuration(_)),
            "base_url {base_url:?}: expected Configuration, got {err:?}"
        );
    }
}

#[test]
fn normalizes_root_url_keeps_single_slash() {
    let client = build_ok(Honcho::builder().base_url("http://localhost:8000").build());
    assert_eq!(client.base_url().as_str(), "http://localhost:8000/");
}

#[test]
fn normalizes_strips_trailing_slash_on_subpath() {
    let client = build_ok(
        Honcho::builder()
            .base_url("http://localhost:8000/api/")
            .build(),
    );
    assert_eq!(client.base_url().as_str(), "http://localhost:8000/api");
}

#[test]
fn normalizes_preserves_query_string() {
    // The trailing-slash trim touches only the path component; the query
    // string is left intact.
    let client = build_ok(
        Honcho::builder()
            .base_url("http://localhost:8000/api/?foo=bar")
            .build(),
    );
    assert_eq!(
        client.base_url().as_str(),
        "http://localhost:8000/api?foo=bar"
    );
}

#[test]
fn normalizes_preserves_fragment() {
    let client = build_ok(
        Honcho::builder()
            .base_url("http://localhost:8000/api/#frag")
            .build(),
    );
    assert_eq!(client.base_url().as_str(), "http://localhost:8000/api#frag");
}

// ── env-driven resolution (serialized + ambient-pinned) ─────────────────

#[test]
#[serial_test::serial]
fn workspace_id_defaults_when_unset() {
    let client = temp_env::with_vars([("HONCHO_WORKSPACE_ID", None::<&str>)], || {
        build_ok(Honcho::builder().base_url("http://localhost:8000").build())
    });
    assert_eq!(client.workspace_id(), "default");
    assert_eq!(client.base_url().as_str(), "http://localhost:8000/");
}

#[test]
#[serial_test::serial]
fn workspace_id_arg_overrides_env() {
    let client = temp_env::with_vars(
        [
            ("HONCHO_WORKSPACE_ID", Some("env-workspace")),
            ("HONCHO_URL", None),
            ("HONCHO_API_URL", None),
            ("HONCHO_API_KEY", None),
        ],
        || {
            build_ok(
                Honcho::builder()
                    .base_url("http://localhost:8000")
                    .workspace_id("arg-workspace")
                    .build(),
            )
        },
    );
    assert_eq!(client.workspace_id(), "arg-workspace");
}

#[test]
#[serial_test::serial]
fn builds_from_env_when_no_args_given() {
    let client = temp_env::with_vars(
        [
            ("HONCHO_URL", Some("http://env-host:8000")),
            ("HONCHO_API_URL", None),
            ("HONCHO_WORKSPACE_ID", Some("env-workspace")),
            ("HONCHO_API_KEY", None),
        ],
        || build_ok(Honcho::builder().build()),
    );
    assert_eq!(client.base_url().as_str(), "http://env-host:8000/");
    assert_eq!(client.workspace_id(), "env-workspace");
}

#[test]
#[serial_test::serial]
fn environment_local_base_url() {
    // Pin the URL env vars off: otherwise an ambient HONCHO_URL/HONCHO_API_URL
    // would outrank `Environment::Local` and silently break the assertion.
    let client = temp_env::with_vars(
        [("HONCHO_URL", None::<&str>), ("HONCHO_API_URL", None)],
        || build_ok(Honcho::builder().environment(Environment::Local).build()),
    );
    assert_eq!(client.base_url().as_str(), "http://localhost:8000/");
}

#[test]
#[serial_test::serial]
fn environment_production_is_the_default_base_url() {
    let client = temp_env::with_vars(
        [
            ("HONCHO_URL", None::<&str>),
            ("HONCHO_API_URL", None),
            ("HONCHO_WORKSPACE_ID", None),
            ("HONCHO_API_KEY", None),
        ],
        || build_ok(Honcho::builder().build()),
    );
    assert_eq!(client.base_url().as_str(), "https://api.honcho.dev/");
    assert_eq!(client.workspace_id(), "default");
}

#[test]
#[serial_test::serial]
fn honcho_url_takes_precedence_over_api_url() {
    let client = temp_env::with_vars(
        [
            ("HONCHO_URL", Some("http://primary:8000")),
            ("HONCHO_API_URL", Some("http://secondary:9000")),
            ("HONCHO_WORKSPACE_ID", None),
            ("HONCHO_API_KEY", None),
        ],
        || build_ok(Honcho::builder().build()),
    );
    assert_eq!(client.base_url().as_str(), "http://primary:8000/");
}

#[test]
#[serial_test::serial]
fn api_url_takes_precedence_over_environment() {
    // `environment` defaults to Production, but HONCHO_API_URL must still win.
    let client = temp_env::with_vars(
        [
            ("HONCHO_URL", None::<&str>),
            ("HONCHO_API_URL", Some("http://fallback:8000")),
            ("HONCHO_WORKSPACE_ID", None),
            ("HONCHO_API_KEY", None),
        ],
        || build_ok(Honcho::builder().build()),
    );
    assert_eq!(client.base_url().as_str(), "http://fallback:8000/");
}

#[test]
#[serial_test::serial]
fn base_url_arg_overrides_env() {
    let client = temp_env::with_vars(
        [
            ("HONCHO_URL", Some("http://env-url:9000")),
            ("HONCHO_API_URL", None),
            ("HONCHO_WORKSPACE_ID", None),
            ("HONCHO_API_KEY", None),
        ],
        || build_ok(Honcho::builder().base_url("http://arg-url:8000").build()),
    );
    assert_eq!(client.base_url().as_str(), "http://arg-url:8000/");
    assert_eq!(client.workspace_id(), "default");
}

// ── builder fields reach the wire (wiremock) ────────────────────────────

#[tokio::test]
async fn builder_with_explicit_api_key_sends_bearer() {
    let server = MockServer::start().await;
    // The header matcher means a missing/wrong token yields no match → 404 →
    // the `delete` errors below; `.expect(1)` double-checks exactly one hit.
    Mock::given(method("DELETE"))
        .and(path("/v3/workspaces/ws-target"))
        .and(header("authorization", "Bearer test-key-abc123"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let client = build_ok(
        Honcho::builder()
            .base_url(server.uri())
            .api_key("test-key-abc123")
            .build(),
    );
    client
        .delete_workspace("ws-target")
        .await
        .expect("delete should succeed and carry the bearer token");
}

#[tokio::test]
#[serial_test::serial]
async fn builder_without_api_key_sends_no_auth_header() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/v3/workspaces/ws-target"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let uri = server.uri();
    // Pin the auth/workspace env off so neither the ambient shell nor a
    // concurrent test can inject a Bearer token or a non-default workspace.
    let client = temp_env::with_vars(
        [
            ("HONCHO_API_KEY", None::<&str>),
            ("HONCHO_WORKSPACE_ID", None),
        ],
        || build_ok(Honcho::builder().base_url(uri.clone()).build()),
    );

    assert_eq!(client.workspace_id(), "default");
    client
        .delete_workspace("ws-target")
        .await
        .expect("delete should succeed");

    let requests = server
        .received_requests()
        .await
        .expect("requests should be recorded");
    assert_eq!(requests.len(), 1, "exactly one request expected");
    assert!(
        requests[0].headers.get("authorization").is_none(),
        "no api key anywhere must mean no Authorization header"
    );
}

#[tokio::test]
async fn builder_default_headers_reach_the_wire() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/v3/workspaces/ws-x"))
        .and(header("x-custom-header", "custom-value"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("x-custom-header"),
        HeaderValue::from_static("custom-value"),
    );
    let client = build_ok(
        Honcho::builder()
            .base_url(server.uri())
            .default_headers(headers)
            .build(),
    );
    client
        .delete_workspace("ws-x")
        .await
        .expect("delete should succeed and carry the custom header");
}

#[tokio::test]
async fn builder_default_query_params_reach_the_wire() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/v3/workspaces/ws-x"))
        .and(query_param("tenant", "acme"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let client = build_ok(
        Honcho::builder()
            .base_url(server.uri())
            .default_query(vec![("tenant".to_string(), "acme".to_string())])
            .build(),
    );
    client
        .delete_workspace("ws-x")
        .await
        .expect("delete should succeed and carry the default query param");
}

#[tokio::test]
async fn builder_uses_custom_http_client() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/v3/workspaces/ws-x"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    // The shared `reqwest::Client` from the common helpers is injected as the
    // custom transport; a successful round-trip proves it is the one used.
    let client = build_ok(
        Honcho::builder()
            .base_url(server.uri())
            .http_client(http_client())
            .build(),
    );
    client
        .delete_workspace("ws-x")
        .await
        .expect("delete should succeed through the custom client");
}

#[tokio::test]
async fn builder_timeout_applies_to_requests() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(2)))
        // The timeout aborts the single attempt before any retry could fire
        // (max_retries(0)), so exactly one request reaches the server.
        .expect(1)
        .mount(&server)
        .await;

    let client = build_ok(
        Honcho::builder()
            .base_url(server.uri())
            .timeout(Duration::from_millis(50))
            .max_retries(0)
            .build(),
    );
    let err = client
        .delete_workspace("ws-x")
        .await
        .expect_err("a 50ms timeout against a 2s delay must elapse");
    assert!(
        matches!(err, HonchoError::Timeout { .. }),
        "expected Timeout, got {err:?}"
    );
}

#[tokio::test]
async fn builder_max_retries_zero_disables_retry() {
    let server = MockServer::start().await;
    // DELETE is idempotent, so 503 is normally retried; max_retries(0) must
    // collapse that to a single attempt (verified by `.expect(1)` on drop).
    Mock::given(method("DELETE"))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&server)
        .await;

    let client = build_ok(
        Honcho::builder()
            .base_url(server.uri())
            .max_retries(0)
            .build(),
    );
    let err = client
        .delete_workspace("ws-x")
        .await
        .expect_err("503 should fail without retry");
    assert!(
        matches!(err, HonchoError::Server { status: 503, .. }),
        "expected Server(503), got {err:?}"
    );
}

#[tokio::test]
async fn builder_max_retries_explicit_count_is_honored() {
    let server = MockServer::start().await;
    // max_retries(1) ⇒ 1 initial try + 1 retry = 2 attempts.
    Mock::given(method("DELETE"))
        .respond_with(ResponseTemplate::new(503))
        .expect(2)
        .mount(&server)
        .await;

    let client = build_ok(
        Honcho::builder()
            .base_url(server.uri())
            .max_retries(1)
            .build(),
    );
    let err = client
        .delete_workspace("ws-x")
        .await
        .expect_err("repeated 503 should fail");
    assert!(
        matches!(err, HonchoError::Server { status: 503, .. }),
        "expected Server(503), got {err:?}"
    );
}
