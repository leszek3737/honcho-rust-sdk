use super::harness::TestReport;
use honcho_ai::Honcho;
use honcho_ai::types::session::{SessionConfiguration, SessionPeerConfig};
use serde_json::json;
use std::collections::HashMap;

#[allow(clippy::similar_names, clippy::too_many_lines)]
pub async fn run(honcho: &Honcho, report: &TestReport) -> honcho_ai::error::Result<()> {
    report.scenario("session");

    // Setup failures are reported here via `report.fail` and the scenario
    // returns `Ok(())`; main does NOT additionally treat it as an abort, so a
    // setup failure is counted exactly once.
    let peer1 = match honcho.peer("sess-peer-1").build().await {
        Ok(p) => p,
        Err(e) => {
            report.fail("session_setup_peers", &e.to_string());
            return Ok(());
        }
    };
    let peer2 = match honcho.peer("sess-peer-2").build().await {
        Ok(p) => p,
        Err(e) => {
            report.fail("session_setup_peers", &e.to_string());
            return Ok(());
        }
    };
    let session = match honcho.session("sess-test-1").build().await {
        Ok(s) => s,
        Err(e) => {
            report.fail("session_setup_session", &e.to_string());
            return Ok(());
        }
    };

    match session.id() {
        "sess-test-1" => report.pass("session_id_accessor"),
        id => report.fail(
            "session_id_accessor",
            &format!("expected sess-test-1, got {id}"),
        ),
    }

    if session.is_active() {
        report.pass("session_is_active");
    } else {
        report.fail("session_is_active", "expected true");
    }

    {
        // No non-flaky structural invariant on a freshly-created session's
        // metadata (it is empty until set later), so this is a "doesn't panic"
        // smoke check.
        let _meta = session.metadata();
        report.pass("session_metadata_accessor");
    }

    {
        // No non-flaky structural invariant on a freshly-created session's
        // configuration (defaults are all None), so this is a "doesn't panic"
        // smoke check.
        let _config = session.configuration();
        report.pass("session_configuration_accessor");
    }

    {
        // A real creation timestamp must be a positive Unix time.
        if session.created_at().timestamp() > 0 {
            report.pass("session_created_at");
        } else {
            report.fail("session_created_at", "created_at timestamp is not positive");
        }
    }

    match session.get_metadata().await {
        Ok(_meta) => report.pass("session_get_metadata"),
        Err(e) => report.fail("session_get_metadata", &e.to_string()),
    }

    {
        let meta = HashMap::from([("smoke".to_owned(), json!("test"))]);
        match session.set_metadata(meta).await {
            Ok(()) => match session.get_metadata().await {
                Ok(got) if got.get("smoke").and_then(|v| v.as_str()) == Some("test") => {
                    report.pass("session_set_metadata");
                }
                Ok(got) => report.fail(
                    "session_set_metadata",
                    &format!("roundtrip mismatch: {got:?}"),
                ),
                Err(e) => report.fail("session_set_metadata", &format!("get after set: {e}")),
            },
            Err(e) => report.fail("session_set_metadata", &e.to_string()),
        }
    }

    match session.get_configuration().await {
        Ok(_config) => report.pass("session_get_configuration"),
        Err(e) => report.fail("session_get_configuration", &e.to_string()),
    }

    // set/get configuration round-trip — verify the value survived, not just
    // that the call returned Ok.
    {
        #[allow(clippy::field_reassign_with_default)]
        let config = {
            // `SessionConfiguration` / `ReasoningConfiguration` are
            // `#[non_exhaustive]` with no builder (SDK gap), so a default-then-
            // assign is the only construction available downstream.
            let mut config = SessionConfiguration::default();
            let mut reasoning = honcho_ai::types::common::ReasoningConfiguration::default();
            reasoning.enabled = Some(true);
            config.reasoning = Some(reasoning);
            config
        };
        match session.set_configuration(&config).await {
            Ok(()) => match session.get_configuration().await {
                Ok(got) if got.reasoning.as_ref().and_then(|r| r.enabled) == Some(true) => {
                    report.pass("session_set_configuration");
                }
                Ok(got) => report.fail(
                    "session_set_configuration",
                    &format!("reasoning.enabled not persisted: {got:?}"),
                ),
                Err(e) => report.fail("session_set_configuration", &format!("get after set: {e}")),
            },
            Err(e) => report.fail("session_set_configuration", &e.to_string()),
        }
    }

    match session.get_configuration_raw().await {
        Ok(_raw) => report.pass("session_get_configuration_raw"),
        Err(e) => report.fail("session_get_configuration_raw", &e.to_string()),
    }

    {
        let raw = HashMap::from([("reasoning".to_owned(), json!({"enabled": true}))]);
        match session.set_configuration_raw(raw).await {
            Ok(()) => report.pass("session_set_configuration_raw"),
            Err(e) => report.fail("session_set_configuration_raw", &e.to_string()),
        }
    }

    // refresh — verify the identity is stable after re-fetch (a refresh that
    // corrupts state would be caught).
    match session.refresh().await {
        Ok(()) => {
            if session.id() == "sess-test-1" {
                report.pass("session_refresh");
            } else {
                report.fail("session_refresh", "session id changed after refresh");
            }
        }
        Err(e) => report.fail("session_refresh", &e.to_string()),
    }

    match session.add_peer("sess-peer-1").await {
        Ok(()) => report.pass("session_add_peer"),
        Err(e) => report.fail("session_add_peer", &e.to_string()),
    }

    match session.add_peers([&peer2]).await {
        Ok(()) => report.pass("session_add_peers"),
        Err(e) => report.fail("session_add_peers", &e.to_string()),
    }

    match session.set_peers([&peer1, &peer2]).await {
        Ok(()) => report.pass("session_set_peers"),
        Err(e) => report.fail("session_set_peers", &e.to_string()),
    }

    match session.remove_peers(["sess-peer-2"]).await {
        Ok(()) => report.pass("session_remove_peers"),
        Err(e) => report.fail("session_remove_peers", &e.to_string()),
    }

    match session.peers().await {
        Ok(peers) if peers.len() == 1 && peers[0].id() == "sess-peer-1" => {
            report.pass("session_peers");
        }
        Ok(peers) if peers.len() == 1 => report.fail(
            "session_peers",
            &format!("expected sess-peer-1, got {}", peers[0].id()),
        ),
        Ok(peers) => report.fail(
            "session_peers",
            &format!("expected 1 peer, got {}", peers.len()),
        ),
        Err(e) => report.fail("session_peers", &e.to_string()),
    }

    match session.get_peer_configuration("sess-peer-1").await {
        Ok(_config) => report.pass("session_get_peer_configuration"),
        Err(e) => report.fail("session_get_peer_configuration", &e.to_string()),
    }

    {
        // `SessionPeerConfig` is `#[non_exhaustive]` without a ctor (SDK gap),
        // so it is constructed via serde from a JSON value.
        // On a serde failure, fail this test only and skip the rest of THIS
        // block; do not `return`, so the ~7 later tests still run.
        let config: Result<SessionPeerConfig, _> = serde_json::from_value(json!({
            "observe_me": true,
            "observe_others": true
        }));
        match config {
            Ok(config) => match session.set_peer_configuration("sess-peer-1", &config).await {
                Ok(()) => report.pass("session_set_peer_configuration"),
                Err(e) => report.fail("session_set_peer_configuration", &e.to_string()),
            },
            Err(e) => report.fail("session_set_peer_configuration", &format!("serde: {e}")),
        }
    }

    {
        // Skip only this block on failure; do not `return`, so later tests run.
        match honcho.session("sess-tmp-delete").build().await {
            Ok(tmp) => match tmp.delete().await {
                Ok(()) => report.pass("session_delete"),
                Err(e) => report.fail("session_delete", &e.to_string()),
            },
            Err(e) => report.fail("session_delete", &format!("create temp: {e}")),
        }
    }

    // clone — verify the clone has a distinct id, not just that it returned Ok.
    match session.clone_session().await {
        Ok(cloned) => {
            if cloned.id() == session.id() {
                report.fail("session_clone_session", "clone shares source id");
            } else {
                report.pass("session_clone_session");
            }
        }
        Err(e) => report.fail("session_clone_session", &e.to_string()),
    }

    {
        // Skip only this block on failure; continue to later tests.
        match peer1.message("msg for clone").build() {
            Ok(msg) => match session.add_messages(vec![msg]).await {
                Ok(added) => match added.first() {
                    Some(first) => match session.clone_session_with_message(first.id()).await {
                        Ok(cloned) => {
                            if cloned.id() == session.id() {
                                report.fail(
                                    "session_clone_session_with_message",
                                    "clone shares source id",
                                );
                            } else {
                                report.pass("session_clone_session_with_message");
                            }
                        }
                        Err(e) => {
                            report.fail("session_clone_session_with_message", &e.to_string());
                        }
                    },
                    None => report.fail(
                        "session_clone_session_with_message",
                        "add_messages returned empty",
                    ),
                },
                Err(e) => report.fail(
                    "session_clone_session_with_message",
                    &format!("add_messages: {e}"),
                ),
            },
            Err(e) => report.fail(
                "session_clone_session_with_message",
                &format!("build message: {e}"),
            ),
        }
    }

    match session.representation("sess-peer-1").await {
        Ok(_rep) => report.pass("session_representation"),
        Err(e) => report.fail("session_representation", &e.to_string()),
    }

    match session
        .representation_builder("sess-peer-1")
        .search_query("test")
        .max_conclusions(5)
        .send()
        .await
    {
        Ok(_rep) => report.pass("session_representation_builder"),
        Err(e) => report.fail("session_representation_builder", &e.to_string()),
    }

    match session.queue_status(None, None).await {
        Ok(_status) => report.pass("session_queue_status"),
        Err(e) => report.fail("session_queue_status", &e.to_string()),
    }

    Ok(())
}
