#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

//! Edge-case integration tests for the Honcho client.
//!
//! Focus areas:
//! * `add_messages` chunking at the 100/101 boundary, including the
//!   [`HonchoError::PartialFailure`] path when a later chunk fails.
//! * Environment-variable resolution (`HONCHO_URL`, `HONCHO_API_URL`,
//!   `HONCHO_WORKSPACE_ID`, `HONCHO_API_KEY`) and precedence vs explicit params.
//! * Custom `reqwest::Client` injection actually reaching the wire.
//!
//! ## Environment isolation
//!
//! Tests that exercise env-var resolution are `#[serial_test::serial]` (the
//! process environment is global mutable state) and pin **every** `HONCHO_*`
//! key the SDK consults via [`honcho_env`] — the var under test is set, all
//! others are removed — so ambient values in CI can never leak in or mask a
//! regression. `set_var`/`remove_var` are `unsafe` under edition 2024 and the
//! crate `forbid`s `unsafe_code`, so `temp_env` (which performs the unsafe env
//! mutation inside its own crate) is the only way to drive these from the test
//! target.

mod common;

use honcho_ai::{Honcho, HonchoError, MessageCreate, Session};
use serde_json::{Value, json};
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PEER_ID: &str = "alice";
const MESSAGES_PATH: &str = "/v3/workspaces/ws1/sessions/sess1/messages";

// ── Fixtures ────────────────────────────────────────────────────────────────

/// Builds a [`Session`] (`sess1` in `ws1`) backed by `server`, consuming exactly
/// one workspace-ensure POST and one session-create POST (both verified on drop).
async fn make_session(server: &MockServer) -> Session {
    common::mount_workspace_ensure(server, 1).await;
    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/sessions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(common::session_response("sess1")))
        .expect(1)
        .mount(server)
        .await;

    common::make_honcho(&server.uri())
        .session("sess1")
        .build()
        .await
        .unwrap()
}

/// `n` minimal messages whose serialized wire shape is exactly
/// `{"content": "m{i}", "peer_id": "alice"}` (all optionals stay `None`).
fn make_messages(n: usize) -> Vec<MessageCreate> {
    (0..n)
        .map(|i| {
            MessageCreate::builder()
                .content(format!("m{i}"))
                .peer_id(PEER_ID)
                .build()
        })
        .collect()
}

/// Expected request body for the chunk covering `range` — mirrors the SDK's
/// `MessageBatchCreate` serialization, so it can drive a `body_json` matcher and
/// distinguish chunk 1 from chunk 2.
fn chunk_body(range: std::ops::Range<usize>) -> Value {
    let messages: Vec<Value> = range
        .map(|i| json!({ "content": format!("m{i}"), "peer_id": PEER_ID }))
        .collect();
    json!({ "messages": messages })
}

/// Mock `Vec<MessageResponse>` response for the chunk covering `range`.
fn chunk_response(range: std::ops::Range<usize>) -> Value {
    let items: Vec<Value> = range
        .map(|i| {
            json!({
                "id": format!("msg{i}"),
                "content": format!("m{i}"),
                "peer_id": PEER_ID,
                "session_id": "sess1",
                "workspace_id": "ws1",
                "metadata": {},
                "created_at": "2025-01-15T10:30:00Z",
                "token_count": 0
            })
        })
        .collect();
    Value::Array(items)
}

// ════════════════════════════════════════════════════════════════════════════
// add_messages: empty input
// ════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn add_messages_empty_vec_skips_request() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    // Any POST here would be a bug: empty input must early-return without
    // touching the network. `expect(0)` is verified when the server drops.
    Mock::given(method("POST"))
        .and(path(MESSAGES_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(0)
        .mount(&server)
        .await;

    let result = session.add_messages(vec![]).await.unwrap();
    assert!(result.is_empty());

    // Belt-and-suspenders: assert directly that nothing hit the messages route,
    // rather than inferring it from a would-be 404 panic on a missing mock.
    let requests = server.received_requests().await.unwrap_or_default();
    assert!(
        !requests.iter().any(|r| r.url.path().ends_with("/messages")),
        "empty add_messages must not POST to the messages endpoint"
    );
}

// ════════════════════════════════════════════════════════════════════════════
// add_messages: chunking at the 100/101 boundary
// ════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn add_messages_single_batch_returns_created_messages() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    Mock::given(method("POST"))
        .and(path(MESSAGES_PATH))
        .and(body_json(chunk_body(0..2)))
        .respond_with(ResponseTemplate::new(200).set_body_json(chunk_response(0..2)))
        .expect(1)
        .mount(&server)
        .await;

    let created = session.add_messages(make_messages(2)).await.unwrap();
    assert_eq!(created.len(), 2);
    assert_eq!(created[0].content(), "m0");
    assert_eq!(created[1].id(), "msg1");
}

#[tokio::test]
async fn add_messages_at_100_sends_single_batch() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    // 100 is the inclusive upper bound of the single-request path: still one POST.
    Mock::given(method("POST"))
        .and(path(MESSAGES_PATH))
        .and(body_json(chunk_body(0..100)))
        .respond_with(ResponseTemplate::new(200).set_body_json(chunk_response(0..100)))
        .expect(1)
        .mount(&server)
        .await;

    let created = session.add_messages(make_messages(100)).await.unwrap();
    assert_eq!(created.len(), 100);
}

#[tokio::test]
async fn add_messages_at_101_chunks_into_two_batches() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    // 101 crosses the boundary: a 100-message chunk followed by a 1-message
    // chunk. The two mocks are disambiguated purely by `body_json`.
    Mock::given(method("POST"))
        .and(path(MESSAGES_PATH))
        .and(body_json(chunk_body(0..100)))
        .respond_with(ResponseTemplate::new(200).set_body_json(chunk_response(0..100)))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(MESSAGES_PATH))
        .and(body_json(chunk_body(100..101)))
        .respond_with(ResponseTemplate::new(200).set_body_json(chunk_response(100..101)))
        .expect(1)
        .mount(&server)
        .await;

    let created = session.add_messages(make_messages(101)).await.unwrap();
    assert_eq!(created.len(), 101);
    // Order is preserved across chunks: the 101st item comes from chunk 2.
    assert_eq!(created[100].content(), "m100");
}

#[tokio::test]
async fn add_messages_partial_failure_when_second_chunk_fails() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    // Chunk 1 (100 msgs) succeeds, chunk 2 (1 msg) 500s. POST is non-idempotent,
    // so the SDK does not retry it — each chunk mock is hit exactly once.
    Mock::given(method("POST"))
        .and(path(MESSAGES_PATH))
        .and(body_json(chunk_body(0..100)))
        .respond_with(ResponseTemplate::new(200).set_body_json(chunk_response(0..100)))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(MESSAGES_PATH))
        .and(body_json(chunk_body(100..101)))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({ "error": "boom" })))
        .expect(1)
        .mount(&server)
        .await;

    let err = session.add_messages(make_messages(101)).await.unwrap_err();
    match err {
        HonchoError::PartialFailure {
            sent,
            messages,
            error,
        } => {
            assert_eq!(sent, 100);
            assert_eq!(
                messages.len(),
                100,
                "the first chunk's 100 created messages must be preserved"
            );
            assert_eq!(messages[0].content(), "m0");
            assert_eq!(error.status_code(), Some(500));
            assert!(
                matches!(*error, HonchoError::Server { status: 500, .. }),
                "underlying error should be the 500 server error, got {error:?}"
            );
        }
        other => panic!("expected PartialFailure, got {other:?}"),
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Custom reqwest::Client injection
// ════════════════════════════════════════════════════════════════════════════

/// Unique value for a default header the SDK never sets itself; if it reaches
/// the server, the injected client must be the one issuing requests.
const CUSTOM_CLIENT_MARKER: &str = "agent-a7-custom-client";

#[tokio::test]
async fn custom_http_client_is_actually_used() {
    let server = MockServer::start().await;

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "x-honcho-test-client",
        reqwest::header::HeaderValue::from_static(CUSTOM_CLIENT_MARKER),
    );
    let client = reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .unwrap();

    // The matcher only passes if the request carries the custom client's default
    // header — drop `.http_client(client)` below and this mock never matches,
    // so `force_ensure` 404s and the test fails (the old `is_ok()` check passed
    // regardless of whether the client was actually wired in).
    Mock::given(method("POST"))
        .and(path("/v3/workspaces"))
        .and(header("x-honcho-test-client", CUSTOM_CLIENT_MARKER))
        .respond_with(ResponseTemplate::new(200).set_body_json(common::workspace_response("ws1")))
        .expect(1)
        .mount(&server)
        .await;

    let params = Honcho::builder()
        .base_url(server.uri())
        .workspace_id("ws1")
        .api_key("my-key")
        .http_client(client)
        .build();
    let honcho = Honcho::from_params(params).unwrap();

    honcho.force_ensure().await.unwrap();
}

// ════════════════════════════════════════════════════════════════════════════
// Configuration validation (no env, no server)
// ════════════════════════════════════════════════════════════════════════════

/// Extracts the error from a failed client construction. Hand-rolled because
/// `Honcho` is not `Debug`, so `Result::unwrap_err` is unavailable.
fn expect_construction_err(result: honcho_ai::error::Result<Honcho>) -> HonchoError {
    match result {
        Ok(_) => panic!("expected client construction to fail"),
        Err(err) => err,
    }
}

#[test]
fn invalid_api_key_with_newline_fails() {
    // All params explicit ⇒ no env is consulted, so this is robust without
    // serial isolation.
    let params = Honcho::builder()
        .base_url("http://localhost:1234")
        .workspace_id("ws1")
        .api_key("key\nwith\nnewlines")
        .build();

    let err = expect_construction_err(Honcho::from_params(params));
    assert_eq!(err.code(), "configuration_error");
    assert!(
        matches!(err, HonchoError::Configuration(_)),
        "expected Configuration, got {err:?}"
    );
    assert!(
        err.message().contains("invalid api_key"),
        "message was: {}",
        err.message()
    );
}

#[test]
#[serial_test::serial]
fn invalid_workspace_id_rejected() {
    temp_env::with_vars(honcho_env(&[]), || {
        let params = Honcho::builder()
            .base_url("http://localhost:1234")
            .workspace_id("bad id") // space violates [a-zA-Z0-9_-]+
            .build();

        let err = expect_construction_err(Honcho::from_params(params));
        assert_eq!(err.code(), "configuration_error");
        assert!(
            matches!(err, HonchoError::Configuration(_)),
            "expected Configuration, got {err:?}"
        );
        assert!(
            err.message().contains("[a-zA-Z0-9_-]"),
            "message was: {}",
            err.message()
        );
    });
}

// ════════════════════════════════════════════════════════════════════════════
// 409 Conflict on workspace ensure is treated as success
// ════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn workspace_ensure_conflict_is_ok() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces"))
        .and(body_json(json!({ "id": "ws1" })))
        .respond_with(
            ResponseTemplate::new(409).set_body_json(json!({ "detail": "already exists" })),
        )
        .expect(1)
        .mount(&server)
        .await;

    // make_honcho passes every param explicitly ⇒ immune to ambient HONCHO_*.
    common::make_honcho(&server.uri())
        .force_ensure()
        .await
        .unwrap();
}

// ════════════════════════════════════════════════════════════════════════════
// Environment-variable resolution
// ════════════════════════════════════════════════════════════════════════════

/// Full `HONCHO_*` override set for [`temp_env::with_vars`]: every key the SDK
/// consults is present — set to `Some` when listed in `overrides`, otherwise
/// removed (`None`) — so ambient CI values can neither leak in nor mask a
/// regression.
fn honcho_env(overrides: &[(&str, &str)]) -> Vec<(&'static str, Option<String>)> {
    const KEYS: [&str; 4] = [
        "HONCHO_API_URL",
        "HONCHO_URL",
        "HONCHO_WORKSPACE_ID",
        "HONCHO_API_KEY",
    ];
    KEYS.iter()
        .map(|&key| {
            let value = overrides
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| (*v).to_string());
            (key, value)
        })
        .collect()
}

/// Shared template for env-resolution tests: spins up a mock server expecting a
/// single workspace-ensure POST, pins the full `HONCHO_*` set (`env_for(uri)`),
/// builds the client (`build(uri)`) and asserts the ensure succeeds — which can
/// only happen if env resolution routed the request to this server and matched
/// the `{"id": "ws1"}` body.
///
/// Call sites add `#[serial_test::serial]`.
fn run_env_ensure(
    env_for: impl FnOnce(&str) -> Vec<(&'static str, Option<String>)>,
    build: impl FnOnce(&str) -> honcho_ai::error::Result<Honcho>,
) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let server = rt.block_on(MockServer::start());
    rt.block_on(common::mount_workspace_ensure(&server, 1));
    let uri = server.uri();

    temp_env::with_vars(env_for(&uri), || {
        let honcho = build(&uri).unwrap();
        rt.block_on(honcho.force_ensure()).unwrap();
    });
}

#[test]
#[serial_test::serial]
fn env_honcho_url_resolves_base_url() {
    // base_url comes from HONCHO_URL; workspace_id from the builder.
    run_env_ensure(
        |uri| honcho_env(&[("HONCHO_URL", uri)]),
        |_uri| Honcho::from_params(Honcho::builder().workspace_id("ws1").build()),
    );
}

#[test]
#[serial_test::serial]
fn env_honcho_api_url_resolves_base_url() {
    // HONCHO_API_URL is the lowest-precedence base_url source.
    run_env_ensure(
        |uri| honcho_env(&[("HONCHO_API_URL", uri)]),
        |_uri| Honcho::from_params(Honcho::builder().workspace_id("ws1").build()),
    );
}

#[test]
#[serial_test::serial]
fn env_honcho_url_beats_api_url() {
    // Both env vars set: HONCHO_URL (the real server) must win over the bogus
    // HONCHO_API_URL. If precedence were reversed, the request would hit a dead
    // port and `force_ensure` would error.
    run_env_ensure(
        |uri| {
            honcho_env(&[
                ("HONCHO_URL", uri),
                ("HONCHO_API_URL", "http://127.0.0.1:9"),
            ])
        },
        |_uri| Honcho::from_params(Honcho::builder().workspace_id("ws1").build()),
    );
}

#[test]
#[serial_test::serial]
fn env_honcho_workspace_id_resolves() {
    // workspace_id comes from HONCHO_WORKSPACE_ID; base_url from the builder.
    // The ensure body `{"id": "ws1"}` is the oracle: if env were ignored the
    // body would be `{"id": "default"}` and the mock would not match.
    run_env_ensure(
        |_uri| honcho_env(&[("HONCHO_WORKSPACE_ID", "ws1")]),
        |uri| Honcho::from_params(Honcho::builder().base_url(uri).build()),
    );
}

#[test]
#[serial_test::serial]
fn explicit_base_url_beats_env() {
    // Explicit builder base_url (the real server) must take precedence over the
    // bogus HONCHO_URL.
    run_env_ensure(
        |_uri| honcho_env(&[("HONCHO_URL", "http://127.0.0.1:9")]),
        |uri| Honcho::from_params(Honcho::builder().base_url(uri).workspace_id("ws1").build()),
    );
}

#[test]
#[serial_test::serial]
fn env_honcho_api_key_sends_bearer() {
    // Resolves base_url, workspace_id and api_key entirely from the environment;
    // the mock requires all three, so a miss on any one 404s the ensure.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let server = rt.block_on(MockServer::start());
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/v3/workspaces"))
            .and(body_json(json!({ "id": "ws1" })))
            .and(header("authorization", "Bearer test-secret-key"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(common::workspace_response("ws1")),
            )
            .expect(1)
            .mount(&server),
    );
    let uri = server.uri();

    temp_env::with_vars(
        honcho_env(&[
            ("HONCHO_URL", uri.as_str()),
            ("HONCHO_WORKSPACE_ID", "ws1"),
            ("HONCHO_API_KEY", "test-secret-key"),
        ]),
        || {
            let honcho = Honcho::from_params(Honcho::builder().build()).unwrap();
            rt.block_on(honcho.force_ensure()).unwrap();
        },
    );
}
