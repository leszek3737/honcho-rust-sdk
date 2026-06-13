//! Regression tests for the blocking-API runtime guard.
//!
//! These tests pin the non-obvious semantics documented in
//! [`crate::blocking::runtime`]: the guard keys off
//! `tokio::runtime::Handle::try_current()`, which is `Ok` both on async-driver
//! threads (where nesting `block_on` would panic) and inside `spawn_blocking`
//! (where it would in fact be legal). The guard is intentionally conservative,
//! so the tests assert both directions: rejection inside any tokio context, and
//! a clean pass on a plain OS thread.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

#[cfg(feature = "blocking")]
mod blocking {
    use honcho_ai::blocking::Honcho;
    use honcho_ai::error::HonchoError;
    use serde_json::json;
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Build a blocking client pointed at a bogus port.
    ///
    /// Construction only parses the URL — it never drives the internal runtime
    /// — so it is safe to call from any context (including inside an async
    /// runtime) and never trips the guard.
    fn make_client() -> Honcho {
        Honcho::new("http://localhost:9999", "ws").expect("client construction should succeed")
    }

    /// Workspace-ensure response shape used by the wiremock-backed test.
    fn workspace_response() -> serde_json::Value {
        json!({
            "id": "ws",
            "metadata": {},
            "configuration": {},
            "created_at": "2025-01-15T10:30:00Z"
        })
    }

    // ── (4) renamed: construction is a pure URL parse, not a runtime probe ──

    #[test]
    fn honcho_new_succeeds_outside_runtime() {
        let honcho = make_client();
        assert_eq!(honcho.workspace_id(), "ws");
    }

    // ── (3) fixed: assert on the canonical Configuration message, not a
    //         fragile substring that drifted when the message was canonicalised.

    #[tokio::test]
    async fn blocking_force_ensure_inside_async_returns_error() {
        let honcho = make_client();
        let err = honcho.force_ensure().unwrap_err();
        assert!(
            matches!(err, HonchoError::Configuration(ref m) if m.contains("async runtime")),
            "expected Configuration(async runtime) error, got {err:?}"
        );
    }

    // ── (1) NEW: spawn_blocking still reports an ambient runtime handle, so
    //         the conservative guard trips even though `Runtime::block_on`
    //         would technically be legal there. This documents the
    //         `try_current() == Ok` semantics from the user's perspective.

    #[tokio::test]
    async fn blocking_call_from_spawn_blocking_returns_configuration_error() {
        let honcho = make_client();
        let result = tokio::task::spawn_blocking(move || {
            // Any blocking method that funnels through `block_on` exercises the
            // guard. `force_ensure` is the cheapest such call and returns the
            // guard error before any network I/O when a runtime is ambient.
            honcho.force_ensure()
        })
        .await
        .expect("spawn_blocking task should not panic");

        let err = result.unwrap_err();
        assert!(
            matches!(err, HonchoError::Configuration(ref m) if m.contains("async runtime")),
            "expected Configuration(async runtime) from spawn_blocking, got {err:?}"
        );
    }

    // ── (2) NEW: on a plain OS thread there is no ambient runtime, so the
    //         guard passes and the blocking call reaches the server. Backed by
    //         wiremock so we can assert a true `Ok` rather than a non-Configuration
    //         transport error.

    #[tokio::test]
    async fn blocking_call_from_os_thread_succeeds() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v3/workspaces"))
            .and(body_json(json!({"id": "ws"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(workspace_response()))
            .up_to_n_times(1)
            .mount(&server)
            .await;

        let uri = server.uri();
        // A scoped OS thread has no tokio context bound, so `try_current()`
        // fails and `block_on` drives the future normally.
        let result = std::thread::scope(|s| {
            s.spawn(move || {
                let client = Honcho::new(&uri, "ws").expect("client construction should succeed");
                client.force_ensure()
            })
            .join()
            .expect("os thread should not panic")
        });

        assert!(
            result.is_ok(),
            "expected Ok from raw OS thread (guard should pass), got {result:?}"
        );
    }
}
