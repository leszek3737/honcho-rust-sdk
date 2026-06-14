#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::harness::TestReport;
use honcho_ai::Honcho;
use honcho_ai::types::common::{
    DreamConfiguration, PeerCardConfiguration, ReasoningConfiguration, SummaryConfiguration,
};
use honcho_ai::types::workspace::WorkspaceConfiguration;
use serde_json::json;
use std::collections::HashMap;

#[allow(clippy::too_many_lines)]
pub async fn run(honcho: &Honcho, report: &mut TestReport) {
    report.scenario("workspace");

    let dream_peer = match honcho.peer("dream-peer").build().await {
        Ok(p) => p,
        Err(e) => {
            report.fail("workspace_create_dream_peer", &e.to_string());
            return;
        }
    };
    let _ = dream_peer;

    let name = "workspace_force_ensure";
    match honcho.force_ensure().await {
        Ok(()) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "workspace_id_accessor";
    let ws_id = honcho.workspace_id();
    if ws_id.is_empty() {
        report.fail(name, "workspace_id returned empty string");
    } else {
        report.pass(name);
    }

    let name = "workspace_base_url_accessor";
    let base = honcho.base_url();
    if base.scheme() == "http" || base.scheme() == "https" {
        report.pass(name);
    } else {
        report.fail(name, &format!("unexpected scheme: {}", base.scheme()));
    }

    let name = "workspace_get_metadata";
    match honcho.get_metadata().await {
        Ok(_meta) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "workspace_set_metadata";
    let mut meta = HashMap::new();
    meta.insert("smoke".into(), json!(true));
    meta.insert("env".into(), json!("test"));
    match honcho.set_metadata(meta.clone()).await {
        Ok(()) => match honcho.get_metadata().await {
            Ok(got) => {
                if got.get("smoke") == Some(&json!(true)) && got.get("env") == Some(&json!("test"))
                {
                    report.pass(name);
                } else {
                    report.fail(name, "metadata mismatch after set");
                }
            }
            Err(e) => report.fail(name, &format!("verify get failed: {e}")),
        },
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "workspace_get_configuration";
    match honcho.get_configuration().await {
        Ok(_cfg) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "workspace_set_configuration";
    let mut reasoning = ReasoningConfiguration::default();
    reasoning.enabled = Some(true);
    reasoning.custom_instructions = Some("smoke test instructions".into());

    let mut peer_card = PeerCardConfiguration::default();
    peer_card.use_peer_card = Some(true);
    peer_card.create = Some(true);

    let mut summary = SummaryConfiguration::default();
    summary.enabled = Some(true);
    summary.messages_per_short_summary = Some(10);
    summary.messages_per_long_summary = Some(50);

    let mut dream = DreamConfiguration::default();
    dream.enabled = Some(true);

    let mut config = WorkspaceConfiguration::default();
    config.reasoning = Some(reasoning);
    config.peer_card = Some(peer_card);
    config.summary = Some(summary);
    config.dream = Some(dream);
    match honcho.set_configuration(&config).await {
        Ok(()) => match honcho.get_configuration().await {
            Ok(got) => {
                let reason_ok = got
                    .reasoning
                    .as_ref()
                    .is_some_and(|r| r.enabled == Some(true));
                let summary_ok = got.summary.as_ref().is_some_and(|s| {
                    s.enabled == Some(true) && s.messages_per_short_summary == Some(10)
                });
                if reason_ok && summary_ok {
                    report.pass(name);
                } else {
                    report.fail(name, "configuration mismatch after set");
                }
            }
            Err(e) => report.fail(name, &format!("verify get failed: {e}")),
        },
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "workspace_get_configuration_raw";
    match honcho.get_configuration_raw().await {
        Ok(raw) => {
            if raw.is_empty() {
                report.fail(name, "raw configuration empty");
            } else {
                report.pass(name);
            }
        }
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "workspace_set_configuration_raw";
    let mut raw_cfg = HashMap::new();
    raw_cfg.insert("dream".into(), json!({"enabled": false}));
    match honcho.set_configuration_raw(raw_cfg).await {
        Ok(()) => match honcho.get_configuration_raw().await {
            Ok(got) => {
                if got.contains_key("dream") {
                    report.pass(name);
                } else {
                    report.fail(name, "raw configuration missing dream key after set");
                }
            }
            Err(e) => report.fail(name, &format!("verify raw get failed: {e}")),
        },
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "workspace_refresh";
    match honcho.refresh().await {
        Ok(()) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "workspace_peers_list";
    match honcho.peers().await {
        Ok(_page) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "workspace_peers_with_filters";
    let filters = HashMap::new();
    match honcho.peers_with_filters(filters, 1, 50, false).await {
        Ok(_page) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "workspace_sessions_list";
    match honcho.sessions().await {
        Ok(_page) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "workspace_sessions_with_filters";
    let filters = HashMap::new();
    match honcho.sessions_with_filters(filters, 1, 50, false).await {
        Ok(_page) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "workspace_workspaces_list";
    match honcho.workspaces().await {
        Ok(_page) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "workspace_queue_status";
    match honcho.queue_status(None, None, None).await {
        Ok(status) => {
            let total = status.total_work_units;
            let completed = status.completed_work_units;
            let in_progress = status.in_progress_work_units;
            let pending = status.pending_work_units;
            let sum_valid = total >= completed + in_progress + pending;
            if sum_valid {
                report.pass(name);
            } else {
                report.fail(name, "queue counters inconsistent");
            }
        }
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "workspace_queue_status_filtered";
    match honcho
        .queue_status(
            Some("dream-peer"),
            Some("dream-peer"),
            Some("nonexistent-session"),
        )
        .await
    {
        Ok(_status) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "workspace_schedule_dream";
    match honcho.schedule_dream("dream-peer", None, None).await {
        Ok(()) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "workspace_schedule_dream_with_session";
    match honcho
        .schedule_dream("dream-peer", Some("nonexistent-session"), None)
        .await
    {
        Ok(()) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "honcho_builder_pattern";
    let built = Honcho::builder()
        .base_url("http://localhost:9999".to_owned())
        .workspace_id("builder-test".to_owned())
        .build();
    match Honcho::from_params(built) {
        Ok(client) => {
            if client.workspace_id() == "builder-test"
                && client.base_url().as_str() == "http://localhost:9999/"
            {
                report.pass(name);
            } else {
                report.fail(name, "builder produced wrong workspace_id or base_url");
            }
        }
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "honcho_from_params";
    let params = Honcho::builder()
        .base_url("http://localhost:8888".to_owned())
        .workspace_id("params-test".to_owned())
        .api_key("test-key".to_owned())
        .build();
    match Honcho::from_params(params) {
        Ok(client) => {
            if client.workspace_id() == "params-test" {
                report.pass(name);
            } else {
                report.fail(name, "from_params produced wrong workspace_id");
            }
        }
        Err(e) => report.fail(name, &e.to_string()),
    }
}
