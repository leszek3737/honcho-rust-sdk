#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

use honcho_ai::Honcho;
use serde_json::{Value, json};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn workspace_response_json() -> Value {
    json!({
        "id": "ws1",
        "metadata": {},
        "configuration": {},
        "created_at": "2025-01-15T10:30:00Z"
    })
}

fn session_response_json() -> Value {
    json!({
        "id": "sess1",
        "workspace_id": "ws1",
        "is_active": true,
        "metadata": {},
        "configuration": {},
        "created_at": "2025-01-15T10:30:00Z"
    })
}

async fn make_session(server: &MockServer) -> honcho_ai::Session {
    Mock::given(method("POST"))
        .and(path("/v3/workspaces"))
        .respond_with(ResponseTemplate::new(200).set_body_json(workspace_response_json()))
        .up_to_n_times(1)
        .mount(server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/sessions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(session_response_json()))
        .up_to_n_times(1)
        .mount(server)
        .await;

    let honcho = Honcho::new(&server.uri(), "ws1").unwrap();
    honcho.session("sess1", None, None, None).await.unwrap()
}

// --- Empty vec add_messages (early return) ---

#[tokio::test]
async fn add_messages_empty_vec_returns_empty() {
    let server = MockServer::start().await;
    let session = make_session(&server).await;

    let result = session.add_messages(vec![]).await.unwrap();
    assert!(result.is_empty());
}

// --- Env var resolution: HONCHO_API_KEY -> Authorization header ---

#[test]
#[serial_test::serial]
fn env_var_honcho_api_key_sends_bearer() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = rt.block_on(MockServer::start());

    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/v3/workspaces"))
            .and(header("Authorization", "Bearer test-secret-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(workspace_response_json()))
            .up_to_n_times(1)
            .mount(&server),
    );

    let uri = server.uri();
    temp_env::with_var("HONCHO_API_KEY", Some("test-secret-key"), || {
        let params = Honcho::builder().base_url(&uri).workspace_id("ws1").build();
        let honcho = Honcho::from_params(params).unwrap();
        let result = rt.block_on(honcho.force_ensure());
        assert!(result.is_ok());
    });
}

// --- Env var resolution: HONCHO_URL ---

#[test]
#[serial_test::serial]
fn env_var_honcho_url_resolution() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = rt.block_on(MockServer::start());

    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/v3/workspaces"))
            .respond_with(ResponseTemplate::new(200).set_body_json(workspace_response_json()))
            .up_to_n_times(1)
            .mount(&server),
    );

    let uri = server.uri();
    temp_env::with_var("HONCHO_URL", Some(uri.as_str()), || {
        let params = Honcho::builder().workspace_id("ws1").build();
        let honcho = Honcho::from_params(params).unwrap();
        let result = rt.block_on(honcho.force_ensure());
        assert!(result.is_ok());
    });
}

// --- Invalid api_key (newline chars) ---

#[test]
fn invalid_api_key_with_newline_fails() {
    let params = Honcho::builder()
        .base_url("http://localhost:1234")
        .workspace_id("ws1")
        .api_key("key\nwith\nnewlines")
        .build();
    let result = Honcho::from_params(params);
    assert!(result.is_err());
    if let Err(err) = result {
        assert_eq!(err.code(), "configuration_error");
        assert!(err.message().contains("invalid api_key"));
    }
}

// --- Custom reqwest::Client + api_key ---

#[tokio::test]
async fn custom_reqwest_client_with_api_key() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces"))
        .respond_with(ResponseTemplate::new(200).set_body_json(workspace_response_json()))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let params = Honcho::builder()
        .base_url(server.uri())
        .workspace_id("ws1")
        .api_key("my-key")
        .http_client(client)
        .build();
    let honcho = Honcho::from_params(params).unwrap();

    let result = honcho.force_ensure().await;
    assert!(result.is_ok());
}
