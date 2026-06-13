//! Blocking smoke tests — wiremock-backed tests for every blocking method.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::needless_pass_by_value,
    clippy::uninlined_format_args,
    clippy::manual_range_contains,
    missing_docs
)]

use honcho_ai::blocking::Honcho;
use honcho_ai::types::message::MessageSearchOptions;
use honcho_ai::types::workspace::WorkspaceConfiguration;
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn ws_json() -> serde_json::Value {
    serde_json::json!({
        "id": "ws1",
        "metadata": {},
        "configuration": {},
        "created_at": "2025-01-15T10:30:00Z"
    })
}

fn ws_json_with_config() -> serde_json::Value {
    serde_json::json!({
        "id": "ws1",
        "metadata": {},
        "configuration": {
            "reasoning": {"enabled": true}
        },
        "created_at": "2025-01-15T10:30:00Z"
    })
}

fn peer_json() -> serde_json::Value {
    serde_json::json!({
        "id": "alice",
        "workspace_id": "ws1",
        "created_at": "2025-01-15T10:30:00Z",
        "metadata": {},
        "configuration": {}
    })
}

fn session_json() -> serde_json::Value {
    serde_json::json!({
        "id": "sess1",
        "workspace_id": "ws1",
        "is_active": true,
        "metadata": {},
        "configuration": {},
        "created_at": "2025-01-15T10:30:00Z"
    })
}

fn msg_json(id: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "content": "hello world",
        "peer_id": "alice",
        "session_id": "sess1",
        "metadata": {},
        "created_at": "2025-01-15T10:30:00Z",
        "workspace_id": "ws1",
        "token_count": 2
    })
}

fn context_json() -> serde_json::Value {
    serde_json::json!({
        "id": "sess1",
        "messages": [msg_json("m1")],
        "summary": {
            "content": "a summary",
            "message_id": "msg0",
            "summary_type": "short",
            "created_at": "2025-01-15T10:30:00Z",
            "token_count": 5
        },
        "peer_representation": "some rep",
        "peer_card": ["fact1"]
    })
}

fn page_json(
    items: Vec<serde_json::Value>,
    total: u64,
    page: u64,
    size: u64,
    pages: u64,
) -> serde_json::Value {
    serde_json::json!({
        "items": items,
        "total": total,
        "page": page,
        "size": size,
        "pages": pages
    })
}

fn queue_status_json() -> serde_json::Value {
    serde_json::json!({
        "total_work_units": 5,
        "completed_work_units": 3,
        "in_progress_work_units": 1,
        "pending_work_units": 1
    })
}

async fn mount_ensure_ws(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/v3/workspaces"))
        .and(body_json(serde_json::json!({"id": "ws1"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(ws_json()))
        .up_to_n_times(1)
        .mount(server)
        .await;
}

async fn mount_create_session(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/sessions"))
        .and(body_json(serde_json::json!({"id": "sess1"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(session_json()))
        .up_to_n_times(1)
        .mount(server)
        .await;
}

async fn mount_create_peer(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers"))
        .and(body_json(serde_json::json!({"id": "alice"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(peer_json()))
        .up_to_n_times(1)
        .mount(server)
        .await;
}

fn blocking<F, R>(f: F) -> R
where
    F: FnOnce() -> R + Send,
    R: Send,
{
    // resume_unwind preserves the worker thread's original panic payload/message
    // instead of swallowing it inside a generic join failure.
    std::thread::scope(|s| {
        s.spawn(f)
            .join()
            .unwrap_or_else(|p| std::panic::resume_unwind(p))
    })
}

// ─── Session: context ────────────────────────────────────────────────

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_session_context() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;
    mount_create_session(&server).await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/context"))
        .and(query_param("summary", "true"))
        .and(query_param("limit_to_session", "false"))
        .respond_with(ResponseTemplate::new(200).set_body_json(context_json()))
        .mount(&server)
        .await;

    let uri = server.uri();
    let ctx = blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        let session = client.session("sess1", None, None, None).unwrap();
        session.context().unwrap()
    });
    assert_eq!(ctx.id, "sess1");
    assert_eq!(ctx.messages.len(), 1);
}

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_session_context_with_options() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;
    mount_create_session(&server).await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/context"))
        .and(query_param("summary", "false"))
        .and(query_param("limit_to_session", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(context_json()))
        .mount(&server)
        .await;

    let uri = server.uri();
    let ctx = blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        let session = client.session("sess1", None, None, None).unwrap();
        let opts = honcho_ai::types::session::SessionContextOptions::builder()
            .summary(false)
            .limit_to_session(true)
            .build();
        session.context_with_options(&opts).unwrap()
    });
    assert_eq!(ctx.id, "sess1");
}

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_session_context_builder() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;
    mount_create_session(&server).await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/context"))
        .and(query_param("summary", "false"))
        .and(query_param("limit_to_session", "true"))
        .and(query_param("tokens", "4096"))
        .and(query_param("peer_target", "bob"))
        .and(query_param("peer_perspective", "alice"))
        .and(query_param("search_query", "preferences"))
        .and(query_param("search_top_k", "10"))
        .and(query_param("search_max_distance", "0.5"))
        .and(query_param("include_most_frequent", "true"))
        .and(query_param("max_conclusions", "20"))
        .respond_with(ResponseTemplate::new(200).set_body_json(context_json()))
        .mount(&server)
        .await;

    let uri = server.uri();
    let ctx = blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        let session = client.session("sess1", None, None, None).unwrap();
        session
            .context_builder()
            .summary(false)
            .limit_to_session(true)
            .tokens(4096)
            .peer_target("bob")
            .peer_perspective("alice")
            .search_query("preferences")
            .search_top_k(10)
            .search_max_distance(0.5)
            .include_most_frequent(true)
            .max_conclusions(20)
            .send()
            .unwrap()
    });
    assert_eq!(ctx.id, "sess1");
    assert_eq!(ctx.messages.len(), 1);
}

// ─── Session: summaries ──────────────────────────────────────────────

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_session_summaries() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;
    mount_create_session(&server).await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/summaries"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "sess1",
            "short_summary": {
                "content": "short",
                "message_id": "m0",
                "summary_type": "short",
                "created_at": "2025-01-15T10:30:00Z",
                "token_count": 3
            }
        })))
        .mount(&server)
        .await;

    let uri = server.uri();
    let summaries = blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        let session = client.session("sess1", None, None, None).unwrap();
        session.summaries().unwrap()
    });
    assert_eq!(summaries.id, "sess1");
    assert!(summaries.short_summary.is_some());
    assert_eq!(summaries.short_summary.unwrap().content, "short");
}

// ─── Session: search ─────────────────────────────────────────────────

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_session_search() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;
    mount_create_session(&server).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/search"))
        .and(body_json(serde_json::json!({
            "query": "hello",
            "limit": 10
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(vec![msg_json("m1")]))
        .mount(&server)
        .await;

    let uri = server.uri();
    let results = blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        let session = client.session("sess1", None, None, None).unwrap();
        session.search("hello").unwrap()
    });
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id(), "m1");
}

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_session_search_with_options() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;
    mount_create_session(&server).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/search"))
        .and(body_json(serde_json::json!({
            "query": "hello",
            "limit": 20
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(vec![msg_json("m1")]))
        .mount(&server)
        .await;

    let uri = server.uri();
    let results = blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        let session = client.session("sess1", None, None, None).unwrap();
        session
            .search_with_options(&MessageSearchOptions {
                query: "hello".into(),
                filters: None,
                limit: 20,
            })
            .unwrap()
    });
    assert_eq!(results.len(), 1);
}

// ─── Session: representation ─────────────────────────────────────────

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_session_representation() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;
    mount_create_session(&server).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/alice/representation"))
        .and(body_json(serde_json::json!({"session_id": "sess1"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "representation": "Alice likes Rust"
        })))
        .mount(&server)
        .await;

    let uri = server.uri();
    let rep = blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        let session = client.session("sess1", None, None, None).unwrap();
        session.representation("alice").unwrap()
    });
    assert_eq!(rep, "Alice likes Rust");
}

// ─── Session: queue_status ───────────────────────────────────────────

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_session_queue_status() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;
    mount_create_session(&server).await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/queue/status"))
        .and(query_param("session_id", "sess1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(queue_status_json()))
        .mount(&server)
        .await;

    let uri = server.uri();
    let status = blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        let session = client.session("sess1", None, None, None).unwrap();
        session.queue_status(None, None).unwrap()
    });
    assert_eq!(status.total_work_units, 5);
    assert_eq!(status.completed_work_units, 3);
}

// ─── Session: messages ───────────────────────────────────────────────

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_session_messages() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;
    mount_create_session(&server).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/messages/list"))
        .and(query_param("page", "1"))
        .and(query_param("size", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page_json(
            vec![msg_json("m1")],
            1,
            1,
            50,
            1,
        )))
        .mount(&server)
        .await;

    let uri = server.uri();
    let msgs = blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        let session = client.session("sess1", None, None, None).unwrap();
        session.messages().unwrap()
    });
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].id(), "m1");
}

// ─── Peer: search ────────────────────────────────────────────────────

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_peer_search() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;
    mount_create_peer(&server).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/alice/search"))
        .and(body_json(serde_json::json!({
            "query": "hello",
            "limit": 10
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(vec![msg_json("m1")]))
        .mount(&server)
        .await;

    let uri = server.uri();
    let results = blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        let peer = client.peer("alice", None, None).unwrap();
        peer.search("hello").unwrap()
    });
    assert_eq!(results.len(), 1);
}

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_peer_search_with_options() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;
    mount_create_peer(&server).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/alice/search"))
        .and(body_json(serde_json::json!({
            "query": "hello",
            "limit": 25
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(vec![msg_json("m1")]))
        .mount(&server)
        .await;

    let uri = server.uri();
    let results = blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        let peer = client.peer("alice", None, None).unwrap();
        peer.search_with_options(&MessageSearchOptions {
            query: "hello".into(),
            filters: None,
            limit: 25,
        })
        .unwrap()
    });
    assert_eq!(results.len(), 1);
}

// ─── Peer: context ───────────────────────────────────────────────────

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_peer_context() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;
    mount_create_peer(&server).await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/peers/alice/context"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "peer_id": "alice",
            "target_id": "alice",
            "representation": "curious mind",
            "peer_card": ["friendly"]
        })))
        .mount(&server)
        .await;

    let uri = server.uri();
    let ctx = blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        let peer = client.peer("alice", None, None).unwrap();
        peer.context().unwrap()
    });
    assert_eq!(ctx.peer_id, "alice");
    assert_eq!(ctx.representation.as_deref(), Some("curious mind"));
}

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_peer_context_with_target() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;
    mount_create_peer(&server).await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/peers/alice/context"))
        .and(query_param("target", "bob"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "peer_id": "alice",
            "target_id": "bob",
            "representation": "Bob helps"
        })))
        .mount(&server)
        .await;

    let uri = server.uri();
    let ctx = blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        let peer = client.peer("alice", None, None).unwrap();
        peer.context_builder().target("bob").send().unwrap()
    });
    assert_eq!(ctx.target_id, "bob");
}

// ─── Peer: sessions ──────────────────────────────────────────────────

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_peer_sessions() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;
    mount_create_peer(&server).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/alice/sessions"))
        .and(query_param("page", "1"))
        .and(query_param("size", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page_json(
            vec![session_json()],
            1,
            1,
            50,
            1,
        )))
        .mount(&server)
        .await;

    let uri = server.uri();
    let sessions = blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        let peer = client.peer("alice", None, None).unwrap();
        peer.sessions().unwrap()
    });
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "sess1");
}

// ─── Peer: representation ────────────────────────────────────────────

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_peer_representation() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;
    mount_create_peer(&server).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/alice/representation"))
        .and(body_json(serde_json::json!({})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "representation": "Alice likes cats"
        })))
        .mount(&server)
        .await;

    let uri = server.uri();
    let rep = blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        let peer = client.peer("alice", None, None).unwrap();
        peer.representation().unwrap()
    });
    assert_eq!(rep, "Alice likes cats");
}

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_peer_representation_builder_with_options() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;
    mount_create_peer(&server).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/alice/representation"))
        .and(body_json(serde_json::json!({
            "search_query": "hobbies",
            "search_top_k": 10,
            "search_max_distance": 0.5,
            "include_most_frequent": true,
            "max_conclusions": 25
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "representation": "curated hobbies"
        })))
        .mount(&server)
        .await;

    let uri = server.uri();
    let rep = blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        let peer = client.peer("alice", None, None).unwrap();
        peer.representation_builder()
            .search_query("hobbies")
            .search_top_k(10)
            .search_max_distance(0.5)
            .include_most_frequent(true)
            .max_conclusions(25)
            .send()
            .unwrap()
    });
    assert_eq!(rep, "curated hobbies");
}

// ─── Client: get_configuration ───────────────────────────────────────

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_client_get_configuration() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ws_json_with_config()))
        .mount(&server)
        .await;

    let uri = server.uri();
    let config = blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        client.get_configuration().unwrap()
    });
    assert!(config.reasoning.is_some());
    assert_eq!(config.reasoning.unwrap().enabled, Some(true));
}

// ─── Client: set_configuration ───────────────────────────────────────

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_client_set_configuration() {
    let server = MockServer::start().await;

    Mock::given(method("PUT"))
        .and(path("/v3/workspaces/ws1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ws_json_with_config()))
        .expect(1)
        .mount(&server)
        .await;

    let uri = server.uri();
    blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        let config: WorkspaceConfiguration = serde_json::from_value(serde_json::json!({
            "reasoning": {"enabled": true}
        }))
        .unwrap();
        client.set_configuration(&config).unwrap();
    });
}

// ─── Client: search ──────────────────────────────────────────────────

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_client_search() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/search"))
        .and(body_json(serde_json::json!({
            "query": "hello",
            "limit": 10
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(vec![msg_json("m1")]))
        .mount(&server)
        .await;

    let uri = server.uri();
    let results = blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        client.search("hello", None, None).unwrap()
    });
    assert_eq!(results.len(), 1);
}

// ─── Client: queue_status ────────────────────────────────────────────

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_client_queue_status() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1/queue/status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(queue_status_json()))
        .mount(&server)
        .await;

    let uri = server.uri();
    let status = blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        client.queue_status(None, None, None).unwrap()
    });
    assert_eq!(status.total_work_units, 5);
}

// ─── Client: schedule_dream ──────────────────────────────────────────

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_client_schedule_dream() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/schedule_dream"))
        .and(body_json(serde_json::json!({
            "observer": "alice",
            "observed": "alice",
            "dream_type": "omni"
        })))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let uri = server.uri();
    blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        client.schedule_dream("alice", None, None).unwrap();
    });
}

// ─── Client: peers_with_filters ──────────────────────────────────────

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_client_peers_with_filters() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/list"))
        .and(query_param("page", "1"))
        .and(query_param("size", "10"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page_json(
            vec![peer_json()],
            1,
            1,
            10,
            1,
        )))
        .mount(&server)
        .await;

    let uri = server.uri();
    let peers = blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        client
            .peers_with_filters(std::collections::HashMap::new(), 1, 10, false)
            .unwrap()
    });
    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0].id, "alice");
}

// ─── Client: sessions_with_filters ───────────────────────────────────

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_client_sessions_with_filters() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/sessions/list"))
        .and(query_param("page", "1"))
        .and(query_param("size", "10"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page_json(
            vec![session_json()],
            1,
            1,
            10,
            1,
        )))
        .mount(&server)
        .await;

    let uri = server.uri();
    let sessions = blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        client
            .sessions_with_filters(std::collections::HashMap::new(), 1, 10, false)
            .unwrap()
    });
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "sess1");
}

// ─── Client: workspaces ──────────────────────────────────────────────

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_client_workspaces() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/list"))
        .and(query_param("page", "1"))
        .and(query_param("size", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page_json(
            vec![
                serde_json::json!({"id": "ws_a", "metadata": {}, "configuration": {}, "created_at": "2025-01-15T10:30:00Z"}),
                serde_json::json!({"id": "ws_b", "metadata": {}, "configuration": {}, "created_at": "2025-01-15T10:30:00Z"}),
            ],
            2, 1, 50, 1,
        )))
        .mount(&server)
        .await;

    let uri = server.uri();
    let ws = blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        client.workspaces().unwrap()
    });
    assert_eq!(ws.len(), 2);
    assert_eq!(ws[0], "ws_a");
    assert_eq!(ws[1], "ws_b");
}

// ─── Session: search validates empty query ───────────────────────────

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_session_search_validates_empty() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;
    mount_create_session(&server).await;

    let uri = server.uri();
    let err = blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        let session = client.session("sess1", None, None, None).unwrap();
        session.search("").unwrap_err()
    });
    assert_eq!(err.code(), "validation_error");
}

// ─── Peer: search validates empty query ──────────────────────────────

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_peer_search_validates_empty() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;
    mount_create_peer(&server).await;

    let uri = server.uri();
    let err = blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        let peer = client.peer("alice", None, None).unwrap();
        peer.search("").unwrap_err()
    });
    assert_eq!(err.code(), "validation_error");
}

// ─── Client: delete_workspace ────────────────────────────────────────

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_client_delete_workspace() {
    let server = MockServer::start().await;

    Mock::given(method("DELETE"))
        .and(path("/v3/workspaces/old-ws"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let uri = server.uri();
    blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        client.delete_workspace("old-ws").unwrap();
    });
}

// ─── Client: get/set metadata ────────────────────────────────────────

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_client_get_metadata() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "ws1",
            "metadata": {"env": "test"},
            "configuration": {},
            "created_at": "2025-01-15T10:30:00Z"
        })))
        .mount(&server)
        .await;

    let uri = server.uri();
    let meta = blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        client.get_metadata().unwrap()
    });
    assert_eq!(meta.get("env").unwrap(), "test");
}

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_client_set_metadata() {
    let server = MockServer::start().await;

    Mock::given(method("PUT"))
        .and(path("/v3/workspaces/ws1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ws_json()))
        .expect(1)
        .mount(&server)
        .await;

    let uri = server.uri();
    blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        let mut meta = std::collections::HashMap::new();
        meta.insert("key".into(), serde_json::json!("value"));
        client.set_metadata(meta).unwrap();
    });
}

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_client_refresh() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/v3/workspaces/ws1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "ws1",
            "metadata": {"env": "test"},
            "configuration": {"reasoning": {"enabled": true}},
            "created_at": "2025-01-15T10:30:00Z"
        })))
        .up_to_n_times(3)
        .expect(1)
        .mount(&server)
        .await;

    let uri = server.uri();
    blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        client.refresh().unwrap();
    });
}

// ─── Conclusion: query, list, delete ─────────────────────────────────

fn conclusion_json(id: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "content": format!("conclusion {id}"),
        "observer_id": "alice",
        "observed_id": "bob",
        "workspace_id": "ws1",
        "created_at": "2025-01-15T10:30:00Z"
    })
}

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_conclusion_query() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;
    mount_create_peer(&server).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/conclusions/query"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(vec![conclusion_json("c1"), conclusion_json("c2")]),
        )
        .expect(1)
        .mount(&server)
        .await;

    let uri = server.uri();
    blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        let peer = client.peer("alice", None, None).unwrap();
        let scope = peer.conclusions();
        let results = scope.query("test query").top_k(5).send().unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id(), "c1");
        assert_eq!(results[1].id(), "c2");
    });
}

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_conclusion_list() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;
    mount_create_peer(&server).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/conclusions/list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page_json(
            vec![conclusion_json("c1")],
            1,
            1,
            50,
            1,
        )))
        .expect(1)
        .mount(&server)
        .await;

    let uri = server.uri();
    blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        let peer = client.peer("alice", None, None).unwrap();
        let scope = peer.conclusions();
        let page = scope.list().send().unwrap();
        assert_eq!(page.items().len(), 1);
    });
}

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_conclusion_delete() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;
    mount_create_peer(&server).await;

    Mock::given(method("DELETE"))
        .and(path("/v3/workspaces/ws1/conclusions/c1"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let uri = server.uri();
    blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        let peer = client.peer("alice", None, None).unwrap();
        let scope = peer.conclusions();
        scope.delete("c1").unwrap();
    });
}

// ─── Session: clone, clone_with_message, representation_builder ──────

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_session_clone() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;
    mount_create_session(&server).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/clone"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "sess1-clone",
            "workspace_id": "ws1",
            "is_active": true,
            "metadata": {},
            "configuration": {},
            "created_at": "2025-01-15T10:30:00Z"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let uri = server.uri();
    blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        let session = client.session("sess1", None, None, None).unwrap();
        let cloned = session.clone_session().unwrap();
        assert_eq!(cloned.id(), "sess1-clone");
    });
}

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_session_clone_with_message() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;
    mount_create_session(&server).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/clone"))
        .and(query_param("message_id", "msg1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "sess1-clone",
            "workspace_id": "ws1",
            "is_active": true,
            "metadata": {},
            "configuration": {},
            "created_at": "2025-01-15T10:30:00Z"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let uri = server.uri();
    blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        let session = client.session("sess1", None, None, None).unwrap();
        let cloned = session.clone_session_with_message("msg1").unwrap();
        assert_eq!(cloned.id(), "sess1-clone");
    });
}

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_session_representation_builder() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;
    mount_create_session(&server).await;

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/alice/representation"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "representation": "alice is a user"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let uri = server.uri();
    blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        let session = client.session("sess1", None, None, None).unwrap();
        let rep = session
            .representation_builder("alice")
            .search_query("hobbies")
            .search_top_k(10)
            .send()
            .unwrap();
        assert_eq!(rep, "alice is a user");
    });
}

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_peer_set_configuration_raw() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;
    mount_create_peer(&server).await;

    Mock::given(method("PUT"))
        .and(path("/v3/workspaces/ws1/peers/alice"))
        .respond_with(ResponseTemplate::new(200).set_body_json(peer_json()))
        .expect(1)
        .mount(&server)
        .await;

    let uri = server.uri();
    blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        let peer = client.peer("alice", None, None).unwrap();
        let mut config = std::collections::HashMap::new();
        config.insert("custom".into(), serde_json::json!(42));
        peer.set_configuration_raw(config).unwrap();
    });
}

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_peer_update() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;
    mount_create_peer(&server).await;

    Mock::given(method("PUT"))
        .and(path("/v3/workspaces/ws1/peers/alice"))
        .respond_with(ResponseTemplate::new(200).set_body_json(peer_json()))
        .expect(1)
        .mount(&server)
        .await;

    let uri = server.uri();
    blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        let peer = client.peer("alice", None, None).unwrap();
        let mut meta = std::collections::HashMap::new();
        meta.insert("role".into(), serde_json::json!("admin"));
        peer.update(meta).unwrap();
    });
}

// ─── upload_file_streamed from a plain sync thread ───────────────────
//
// Guards session.rs: `upload_file_streamed` spawn_blocking-outside-runtime
// panic fix. The reader thread now uses std::thread::spawn +
// handle.block_on(async {...}), so constructing + sending from a plain OS
// thread (no tokio runtime) must not panic and must deliver the reader
// bytes in the multipart body.

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_upload_file_streamed_from_sync_thread_succeeds() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;
    mount_create_session(&server).await;

    let payload = b"hello streamed world from a sync thread".to_vec();

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/messages/upload"))
        .and(wiremock::matchers::body_string_contains(
            "hello streamed world from a sync thread",
        ))
        .and(wiremock::matchers::body_string_contains("peer_id"))
        .and(wiremock::matchers::body_string_contains("alice"))
        .respond_with(ResponseTemplate::new(200).set_body_json(vec![msg_json("m_up1")]))
        .expect(1)
        .mount(&server)
        .await;

    let uri = server.uri();
    let payload_for_thread = payload.clone();
    let msgs = blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        let session = client.session("sess1", None, None, None).unwrap();
        let cursor = std::io::Cursor::new(payload_for_thread);
        session
            .upload_file_streamed("doc.txt", cursor, "text/plain")
            .peer("alice")
            .send()
            .unwrap()
    });
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].id(), "m_up1");
}

// ─── truncated upload surfaces Err(Io) ──────────────────────────────
//
// Guards ErrorAwareReader::poll_read EOF=no-growth fix: a Read that yields
// bytes then errors must propagate the io::Error through read_to_end, never
// Ok.

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_upload_file_streamed_truncated_reader_surfaces_io_err() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;
    mount_create_session(&server).await;

    // The mock is never matched because the body stream errors before the
    // request completes, but mounting it ensures wiremock does not return a
    // spurious 404 that would mask the Io error.
    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/sessions/sess1/messages/upload"))
        .respond_with(ResponseTemplate::new(200).set_body_json(vec![msg_json("m_x")]))
        .mount(&server)
        .await;

    let uri = server.uri();
    let err = blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        let session = client.session("sess1", None, None, None).unwrap();
        // Reader: yields 4 bytes of data, then returns a permanent io::Error.
        let reader = FailingReader::new(b"data".to_vec(), 4);
        session
            .upload_file_streamed("doc.txt", reader, "text/plain")
            .peer("alice")
            .send()
            .unwrap_err()
    });
    assert!(
        matches!(err, honcho_ai::error::HonchoError::Io(_)),
        "expected HonchoError::Io, got {err:?}"
    );
}

// ─── no reader-thread leak on early send() failure ──────────────────
//
// Guards session.rs join-on-all-exit-paths: when block_on fails immediately
// (send() called from inside an async runtime), the reader thread is still
// running. send() must join() it before returning — otherwise the thread
// keeps reading the user's reader in the background.
//
// Assertion design: a slow reader (100 ms per byte) carries an AtomicBool
// "dropped" flag set in Drop. If join() is called, send() blocks until the
// thread naturally exits and the reader is dropped → flag is `true` the
// instant send() returns. If join() is skipped, send() returns in <1 ms
// while the thread is still sleeping in read() → flag is `false`.
// A secondary counter-stability check catches lingering threads even if the
// Drop race is lost.

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_upload_file_streamed_joins_reader_thread_on_early_error() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;
    mount_create_session(&server).await;

    let uri = server.uri();
    let dropped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let read_counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let dropped_for_thread = dropped.clone();
    let counter_for_thread = read_counter.clone();
    let dropped_after = dropped.clone();
    let counter_after = read_counter.clone();

    let (result, elapsed, dropped_at_return, counter_at_return) = std::thread::scope(|s| {
        s.spawn(move || -> (honcho_ai::error::Result<Vec<honcho_ai::Message>>, std::time::Duration, bool, usize) {
            // Plain OS thread, no tokio runtime: Honcho/session/upload setup works.
            let client = Honcho::new(&uri, "ws1").unwrap();
            let session = client.session("sess1", None, None, None).unwrap();
            let reader = SlowFiniteReader::new(
                counter_for_thread,
                dropped_for_thread,
                20,
                std::time::Duration::from_millis(100),
            );
            let builder = session
                .upload_file_streamed("doc.txt", reader, "text/plain")
                .peer("alice");

            // Enter an async runtime context: block_on(self.inner.send())
            // inside send() detects the ambient runtime and returns
            // Err(Configuration) immediately. The reader thread is still
            // running at this point.
            let rt = tokio::runtime::Runtime::new().unwrap();
            let start = std::time::Instant::now();
            let result = rt.block_on(async { builder.send() });
            let elapsed = start.elapsed();
            let dropped_at_return = dropped.load(std::sync::atomic::Ordering::SeqCst);
            let counter_at_return = read_counter.load(std::sync::atomic::Ordering::SeqCst);
            (result, elapsed, dropped_at_return, counter_at_return)
        })
        .join()
        .unwrap_or_else(|p| std::panic::resume_unwind(p))
    });

    // send() must return Configuration error (runtime guard fired).
    assert!(
        matches!(result, Err(ref e) if matches!(e, honcho_ai::error::HonchoError::Configuration(_))),
        "expected Configuration error, got {result:?}"
    );

    // join() must have blocked for the reader thread to finish. With the
    // 100 ms per-byte sleep, join() blocks ≥80 ms. Without join(), send()
    // returns in <5 ms and the flag is still false.
    assert!(
        elapsed > std::time::Duration::from_millis(80),
        "join() should have blocked until the reader thread finished; elapsed={elapsed:?}"
    );

    // The reader's Drop flag must be set — proving the thread fully exited
    // (and dropped the reader) BEFORE send() returned.
    assert!(
        dropped_at_return,
        "reader Drop flag not set at send() return — join() was skipped"
    );

    // Secondary: counter must be stable after send() returns.
    std::thread::sleep(std::time::Duration::from_millis(150));
    let counter_after_wait = counter_after.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        counter_after_wait, counter_at_return,
        "reader thread still incrementing after send() returned — join() leaked"
    );

    // And the Drop flag must still be set.
    assert!(
        dropped_after.load(std::sync::atomic::Ordering::SeqCst),
        "reader Drop flag cleared — impossible"
    );
}

// ─── ChatStreamIterator::next from async returns Err, not panic ──────
//
// Guards peer.rs: ChatStreamIterator::next now maps block_on config-
// rejection to Some(Err(Configuration(_))) and is fused via is_complete().
// Previously (iter.rs:50) this panicked.

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_chat_stream_iterator_next_from_async_returns_configuration_err() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;
    mount_create_peer(&server).await;

    let sse_body = format!(
        "{}{}",
        sse_chunk(r#"{"delta":{"content":"hello"}}"#),
        sse_chunk(r#"{"done":true}"#),
    );

    Mock::given(method("POST"))
        .and(path("/v3/workspaces/ws1/peers/alice/chat"))
        .and(body_json(
            serde_json::json!({"query": "hi", "stream": true}),
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(sse_body)
                .insert_header("content-type", "text/event-stream"),
        )
        .expect(1)
        .mount(&server)
        .await;

    let uri = server.uri();

    // Build the iterator from a sync thread (no runtime → send() works).
    let mut iter = blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        let peer = client.peer("alice", None, None).unwrap();
        peer.chat_stream("hi").send().unwrap()
    });

    // We are now inside the #[tokio::test] runtime. Calling next() must NOT
    // panic — it must yield Some(Err(Configuration(_))).
    let first = iter.next();
    let first_err = first.expect("expected Some(Err(Configuration)), got None");
    assert!(
        matches!(first_err, Err(honcho_ai::error::HonchoError::Configuration(ref msg))
            if msg.contains("cannot be used from inside an async runtime")),
        "expected Configuration error, got {first_err:?}"
    );

    // Fuse-after-error: the config-rejection must NOT leave the iterator
    // drivable. A subsequent next() from the SAME async context returns None
    // — no infinite spin that would hang `for`/`collect`/`Iterator::flatten`.
    assert!(
        iter.next().is_none(),
        "fuse-after-error: expected None immediately after the Configuration error"
    );

    // The fuse must be sticky: even from a valid (non-async) thread the
    // iterator stays exhausted — it does not "un-fuse" and re-poll the
    // stream. Before the fix this drain loop spun forever yielding
    // Some(Err(..)); now it terminates immediately.
    std::thread::scope(|s| {
        s.spawn(|| while iter.next().is_some() {})
            .join()
            .unwrap_or_else(|p| std::panic::resume_unwind(p));
    });

    // Fuse: iterator remains exhausted.
    assert!(
        iter.next().is_none(),
        "fuse: expected None after completion"
    );
    assert!(iter.next().is_none(), "fuse: expected None on repeat call");
}

// ─── reader-thread panic surfaces even when send() also fails ────────
//
// Guards session.rs send(): a panic in the reader thread is surfaced
// unconditionally and takes precedence over the send result. A panicking
// reader drops the pipe write half, truncating the body so the (unmounted)
// upload endpoint 404s and `send()` itself returns Err — but the panic is
// the root cause and must win, never be swallowed behind the server error.

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_upload_file_streamed_reader_panic_wins_over_send_error() {
    let server = MockServer::start().await;
    mount_ensure_ws(&server).await;
    mount_create_session(&server).await;
    // Intentionally do NOT mount the upload endpoint: the truncated request
    // 404s, so `send()` returns a server error — yet the reader panic must
    // be the error surfaced.

    let uri = server.uri();
    let err = blocking(move || {
        let client = Honcho::new(&uri, "ws1").unwrap();
        let session = client.session("sess1", None, None, None).unwrap();
        let reader = PanickingReader::new(8);
        // The reader thread's unwind prints to stderr via the default panic
        // hook; that's expected noise for this test. The payload is captured
        // by JoinHandle::join and surfaced as the returned error.
        session
            .upload_file_streamed("doc.txt", reader, "text/plain")
            .peer("alice")
            .send()
            .unwrap_err()
    });
    match err {
        honcho_ai::error::HonchoError::Io(e) => assert!(
            e.to_string().contains("synthetic reader panic"),
            "expected the reader panic message to be surfaced, got: {e}"
        ),
        other => panic!("expected HonchoError::Io carrying the panic message, got {other:?}"),
    }
}

// ─── Test helper readers ─────────────────────────────────────────────

/// Reader that yields `ok_bytes` bytes of filler, then **panics** on the next
/// read with a `&'static str` payload (so `JoinHandle::join` returns it).
struct PanickingReader {
    remaining_ok: usize,
}

impl PanickingReader {
    fn new(ok_bytes: usize) -> Self {
        Self {
            remaining_ok: ok_bytes,
        }
    }
}

impl std::io::Read for PanickingReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        assert!(self.remaining_ok != 0, "synthetic reader panic");
        let n = buf.len().min(self.remaining_ok);
        buf[..n].fill(b'x');
        self.remaining_ok -= n;
        Ok(n)
    }
}

/// Reader that yields the first `fail_at` bytes of `data`, then returns a
/// permanent `io::Error` on the next read.
struct FailingReader {
    data: Vec<u8>,
    pos: usize,
    fail_at: usize,
    errored: bool,
}

impl FailingReader {
    fn new(data: Vec<u8>, fail_at: usize) -> Self {
        Self {
            data,
            pos: 0,
            fail_at,
            errored: false,
        }
    }
}

impl std::io::Read for FailingReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pos >= self.fail_at {
            if self.errored {
                return Ok(0);
            }
            self.errored = true;
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "synthetic truncated-read error",
            ));
        }
        let remaining = self.fail_at - self.pos;
        let to_copy = buf.len().min(remaining);
        buf[..to_copy].copy_from_slice(&self.data[self.pos..self.pos + to_copy]);
        self.pos += to_copy;
        Ok(to_copy)
    }
}

/// Reader that yields one byte per call, sleeping `delay` between reads,
/// up to `total_bytes`. Increments `counter` on every `read()` call.
/// Sets `dropped` to `true` on Drop so callers can verify the reader thread
/// has fully exited.
struct SlowFiniteReader {
    counter: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    dropped: std::sync::Arc<std::sync::atomic::AtomicBool>,
    remaining: usize,
    delay: std::time::Duration,
}

impl SlowFiniteReader {
    fn new(
        counter: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        dropped: std::sync::Arc<std::sync::atomic::AtomicBool>,
        total_bytes: usize,
        delay: std::time::Duration,
    ) -> Self {
        Self {
            counter,
            dropped,
            remaining: total_bytes,
            delay,
        }
    }
}

impl std::io::Read for SlowFiniteReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let _ = self
            .counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.remaining == 0 {
            return Ok(0);
        }
        std::thread::sleep(self.delay);
        self.remaining -= 1;
        if buf.is_empty() {
            return Ok(0);
        }
        buf[0] = b'x';
        Ok(1)
    }
}

impl Drop for SlowFiniteReader {
    fn drop(&mut self) {
        self.dropped
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

/// SSE chunk formatter matching the dialectic stream wire format.
fn sse_chunk(json: &str) -> String {
    format!("data: {json}\n\n")
}
