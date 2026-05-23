use super::harness::TestReport;
use honcho_ai::Honcho;
use honcho_ai::types::session::{SessionConfiguration, SessionPeerConfig};
use serde_json::json;
use std::collections::HashMap;

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::similar_names,
    clippy::too_many_lines
)]
pub async fn run(honcho: &Honcho, report: &mut TestReport) -> honcho_ai::error::Result<()> {
    let peer1 = match honcho.peer("sess-peer-1", None, None).await {
        Ok(p) => p,
        Err(e) => {
            report.fail("session_setup_peers", &e.to_string());
            return Err(e);
        }
    };
    let peer2 = match honcho.peer("sess-peer-2", None, None).await {
        Ok(p) => p,
        Err(e) => {
            report.fail("session_setup_peers", &e.to_string());
            return Err(e);
        }
    };
    let session = match honcho.session("sess-test-1", None, None, None).await {
        Ok(s) => s,
        Err(e) => {
            report.fail("session_setup_session", &e.to_string());
            return Err(e);
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
        let _meta = session.metadata();
        report.pass("session_metadata_accessor");
    }

    {
        let _config = session.configuration();
        report.pass("session_configuration_accessor");
    }

    {
        let _ts = session.created_at();
        report.pass("session_created_at");
    }

    match session.get_metadata().await {
        Ok(_meta) => report.pass("session_get_metadata"),
        Err(e) => report.fail("session_get_metadata", &e.to_string()),
    }

    {
        let mut meta = HashMap::new();
        meta.insert("smoke".into(), json!("test"));
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

    {
        let mut config = SessionConfiguration::default();
        let mut reasoning = honcho_ai::types::common::ReasoningConfiguration::default();
        reasoning.enabled = Some(true);
        config.reasoning = Some(reasoning);
        match session.set_configuration(&config).await {
            Ok(()) => report.pass("session_set_configuration"),
            Err(e) => report.fail("session_set_configuration", &e.to_string()),
        }
    }

    match session.get_configuration_raw().await {
        Ok(_raw) => report.pass("session_get_configuration_raw"),
        Err(e) => report.fail("session_get_configuration_raw", &e.to_string()),
    }

    {
        let mut raw = HashMap::new();
        raw.insert("reasoning".into(), json!({"enabled": true}));
        match session.set_configuration_raw(raw).await {
            Ok(()) => report.pass("session_set_configuration_raw"),
            Err(e) => report.fail("session_set_configuration_raw", &e.to_string()),
        }
    }

    match session.refresh().await {
        Ok(()) => report.pass("session_refresh"),
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
        let config: SessionPeerConfig = match serde_json::from_value(serde_json::json!({
            "observe_me": true,
            "observe_others": true
        })) {
            Ok(c) => c,
            Err(e) => {
                report.fail("session_set_peer_configuration", &format!("serde: {e}"));
                return Ok(());
            }
        };
        match session.set_peer_configuration("sess-peer-1", &config).await {
            Ok(()) => report.pass("session_set_peer_configuration"),
            Err(e) => report.fail("session_set_peer_configuration", &e.to_string()),
        }
    }

    {
        let tmp = match honcho.session("sess-tmp-delete", None, None, None).await {
            Ok(s) => s,
            Err(e) => {
                report.fail("session_delete", &format!("create temp: {e}"));
                return Ok(());
            }
        };
        match tmp.delete().await {
            Ok(()) => report.pass("session_delete"),
            Err(e) => report.fail("session_delete", &e.to_string()),
        }
    }

    match session.clone_session().await {
        Ok(_cloned) => report.pass("session_clone_session"),
        Err(e) => report.fail("session_clone_session", &e.to_string()),
    }

    {
        let msg = peer1.message("msg for clone").build()?;
        let added = match session.add_messages(vec![msg]).await {
            Ok(msgs) => msgs,
            Err(e) => {
                report.fail(
                    "session_clone_session_with_message",
                    &format!("add_messages: {e}"),
                );
                return Ok(());
            }
        };
        if let Some(first) = added.first() {
            match session.clone_session_with_message(first.id()).await {
                Ok(_cloned) => report.pass("session_clone_session_with_message"),
                Err(e) => report.fail("session_clone_session_with_message", &e.to_string()),
            }
        } else {
            report.fail(
                "session_clone_session_with_message",
                "add_messages returned empty",
            );
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
