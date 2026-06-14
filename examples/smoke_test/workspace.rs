use super::harness::TestReport;
use honcho_ai::Honcho;
use honcho_ai::types::common::{
    DreamConfiguration, PeerCardConfiguration, ReasoningConfiguration, SummaryConfiguration,
};
use honcho_ai::types::workspace::WorkspaceConfiguration;
use serde_json::json;
use std::collections::HashMap;

#[allow(clippy::too_many_lines)]
pub async fn run(honcho: &Honcho, report: &TestReport) {
    report.scenario("workspace");

    // The dream peer must exist server-side for the schedule_dream tests below.
    // A failure here is reported but does NOT abort: the local-only tests
    // (builder / from_params) further down must still run, keeping the test
    // count stable across runs.
    let name = "workspace_create_dream_peer";
    match honcho.peer("dream-peer").build().await {
        Ok(_p) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "workspace_force_ensure";
    match honcho.force_ensure().await {
        Ok(()) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "workspace_id_accessor";
    let ws_id = honcho.workspace_id().to_owned();
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
    let meta = HashMap::from([
        ("smoke".to_owned(), json!(true)),
        ("env".to_owned(), json!("test")),
    ]);
    match honcho.set_metadata(meta).await {
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
    // All four config structs are `#[non_exhaustive]` without builders (SDK
    // gap), so each is constructed via default-then-assign.
    #[allow(clippy::field_reassign_with_default)]
    let config = {
        let mut reasoning = ReasoningConfiguration::default();
        reasoning.enabled = Some(true);
        reasoning.custom_instructions = Some("smoke test instructions".to_owned());

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
        config
    };
    match honcho.set_configuration(&config).await {
        Ok(()) => match honcho.get_configuration().await {
            Ok(got) => {
                // Verify all four sub-configs survived, not just two.
                let reason_ok = got.reasoning.as_ref().is_some_and(|r| {
                    r.enabled == Some(true)
                        && r.custom_instructions.as_deref() == Some("smoke test instructions")
                });
                let peer_card_ok = got
                    .peer_card
                    .as_ref()
                    .is_some_and(|p| p.use_peer_card == Some(true) && p.create == Some(true));
                let summary_ok = got.summary.as_ref().is_some_and(|s| {
                    s.enabled == Some(true)
                        && s.messages_per_short_summary == Some(10)
                        && s.messages_per_long_summary == Some(50)
                });
                let dream_ok = got.dream.as_ref().is_some_and(|d| d.enabled == Some(true));
                if reason_ok && peer_card_ok && summary_ok && dream_ok {
                    report.pass(name);
                } else {
                    report.fail(
                        name,
                        &format!(
                            "configuration mismatch: reasoning={reason_ok} peer_card={peer_card_ok} summary={summary_ok} dream={dream_ok}"
                        ),
                    );
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

    // set_configuration_raw is a whole-config PUT. Assert the written *value*,
    // not just that the `dream` key is present (a prior test already set it, so
    // `contains_key` would pass even for a no-op write).
    let name = "workspace_set_configuration_raw";
    let raw_cfg = HashMap::from([("dream".to_owned(), json!({"enabled": false}))]);
    match honcho.set_configuration_raw(raw_cfg).await {
        Ok(()) => match honcho.get_configuration_raw().await {
            Ok(got) => {
                let dream_disabled = got
                    .get("dream")
                    .and_then(|d| d.get("enabled"))
                    .and_then(serde_json::Value::as_bool)
                    == Some(false);
                if dream_disabled {
                    report.pass(name);
                } else {
                    report.fail(
                        name,
                        &format!("dream.enabled not set to false: {:?}", got.get("dream")),
                    );
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
    // Empty filters return all peers, so the "dream-peer" created synchronously
    // above must be present (creation is not search-indexed: deterministic).
    let filters = HashMap::new();
    match honcho.peers_with_filters(filters, 1, 50, false).await {
        Ok(page) => {
            if page.items().iter().any(|p| p.id == "dream-peer") {
                report.pass(name);
            } else {
                report.fail(
                    name,
                    "created peer 'dream-peer' missing from unfiltered list",
                );
            }
        }
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "workspace_sessions_list";
    match honcho.sessions().await {
        Ok(_page) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "workspace_sessions_with_filters";
    // Create a session synchronously so its presence in an unfiltered list is
    // deterministic (creation is not search-indexed), then assert it appears.
    match honcho.session("workspace-filter-sess").build().await {
        Ok(known_session) => {
            let known_session_id = known_session.id().to_owned();
            let filters = HashMap::new();
            match honcho.sessions_with_filters(filters, 1, 50, false).await {
                Ok(page) => {
                    if page.items().iter().any(|s| s.id == known_session_id) {
                        report.pass(name);
                    } else {
                        report.fail(name, "created session missing from unfiltered list");
                    }
                }
                Err(e) => report.fail(name, &e.to_string()),
            }
        }
        Err(e) => report.fail(name, &format!("create session: {e}")),
    }

    // workspaces() must list the current workspace.
    let name = "workspace_workspaces_list";
    match honcho.workspaces().await {
        Ok(page) => {
            if page.items().iter().any(|id| id == &ws_id) {
                report.pass(name);
            } else {
                report.fail(name, "current workspace not present in workspaces list");
            }
        }
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "workspace_queue_status";
    match honcho.queue_status(None, None, None).await {
        Ok(status) => {
            let total = status.total_work_units;
            let completed = status.completed_work_units;
            let in_progress = status.in_progress_work_units;
            let pending = status.pending_work_units;
            // Counters are u64; sum can overflow in debug builds, which would
            // panic the whole smoke run. Use checked_add and treat overflow as
            // an inconsistency. `total >=` (not `==`) because completed work may
            // have been pruned from the live breakdown.
            let parts_sum = completed
                .checked_add(in_progress)
                .and_then(|s| s.checked_add(pending));
            match parts_sum {
                Some(sum) if total >= sum => report.pass(name),
                Some(sum) => report.fail(
                    name,
                    &format!("queue counters inconsistent: total {total} < parts {sum}"),
                ),
                None => report.fail(name, "queue counter sum overflowed u64"),
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
        .base_url("http://localhost:9999")
        .workspace_id("builder-test")
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

    // from_params differs from the builder test by carrying an api_key — assert
    // that distinguishing input takes effect (the client builds successfully
    // with it).
    let name = "honcho_from_params";
    let params = Honcho::builder()
        .base_url("http://localhost:8888")
        .workspace_id("params-test")
        .api_key("test-key")
        .build();
    match Honcho::from_params(params) {
        Ok(client) => {
            if client.workspace_id() == "params-test"
                && client.base_url().as_str() == "http://localhost:8888/"
            {
                report.pass(name);
            } else {
                report.fail(name, "from_params produced wrong workspace_id or base_url");
            }
        }
        Err(e) => report.fail(name, &e.to_string()),
    }
}
