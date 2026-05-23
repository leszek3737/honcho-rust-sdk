#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::harness::TestReport;
use honcho_ai::Honcho;
use honcho_ai::types::message::MessageSearchOptions;
use honcho_ai::types::peer::PeerConfig;
use honcho_ai::types::session::SessionListOptions;
use serde_json::json;
use std::collections::HashMap;

#[allow(clippy::too_many_lines)]
pub async fn run(honcho: &Honcho, report: &mut TestReport) {
    report.scenario("peer");

    let name = "peer_create";
    let peer = match honcho.peer("smoke-peer-1", None, None).await {
        Ok(p) => {
            report.pass(name);
            p
        }
        Err(e) => {
            report.fail(name, &e.to_string());
            return;
        }
    };

    let name = "peer_id_accessor";
    if peer.id() == "smoke-peer-1" {
        report.pass(name);
    } else {
        report.fail(name, &format!("expected smoke-peer-1, got {}", peer.id()));
    }

    let name = "peer_create_with_metadata";
    let mut metadata_map = HashMap::new();
    metadata_map.insert("role".into(), json!("tester"));
    match honcho
        .peer("smoke-peer-2", Some(metadata_map.clone()), None)
        .await
    {
        Ok(_p2) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "peer_get_metadata";
    match peer.get_metadata().await {
        Ok(_meta) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "peer_set_metadata";
    let mut meta = HashMap::new();
    meta.insert("updated".into(), json!(true));
    match peer.set_metadata(meta.clone()).await {
        Ok(()) => match peer.get_metadata().await {
            Ok(got) => {
                let val = got.get("updated");
                if val == Some(&json!(true)) {
                    report.pass(name);
                } else {
                    report.fail(name, "metadata mismatch after set");
                }
            }
            Err(e) => report.fail(name, &format!("verify get failed: {e}")),
        },
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "peer_get_configuration";
    match peer.get_configuration().await {
        Ok(_cfg) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "peer_set_configuration";
    let mut config = PeerConfig::default();
    config.observe_me = Some(true);
    config.observe_others = Some(false);
    match peer.set_configuration(&config).await {
        Ok(()) => match peer.get_configuration().await {
            Ok(got) => {
                if got.observe_me == Some(true) && got.observe_others == Some(false) {
                    report.pass(name);
                } else {
                    report.fail(name, "configuration mismatch after set");
                }
            }
            Err(e) => report.fail(name, &format!("verify get failed: {e}")),
        },
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "peer_get_configuration_raw";
    match peer.get_configuration_raw().await {
        Ok(_raw) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "peer_set_configuration_raw";
    let mut raw_cfg = HashMap::new();
    raw_cfg.insert("observe_me".into(), json!(false));
    raw_cfg.insert("observe_others".into(), json!(true));
    match peer.set_configuration_raw(raw_cfg).await {
        Ok(()) => match peer.get_configuration_raw().await {
            Ok(got) => {
                if got.get("observe_me") == Some(&json!(false)) {
                    report.pass(name);
                } else {
                    report.fail(name, "raw configuration mismatch after set");
                }
            }
            Err(e) => report.fail(name, &format!("verify raw get failed: {e}")),
        },
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "peer_update";
    let mut new_meta = HashMap::new();
    new_meta.insert("patched".into(), json!("yes"));
    match peer.update(new_meta).await {
        Ok(()) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "peer_refresh";
    match peer.refresh().await {
        Ok(()) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "peer_sessions";
    match peer.sessions().await {
        Ok(_page) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "peer_sessions_with_options";
    let opts = SessionListOptions::builder().page(1).size(10).build();
    match peer.sessions_with_options(&opts).await {
        Ok(_page) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "peer_search";
    match peer.search("test").await {
        Ok(_results) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "peer_search_with_options";
    let search_opts = MessageSearchOptions::builder()
        .query("test")
        .limit(5)
        .build();
    match peer.search_with_options(&search_opts).await {
        Ok(_results) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "peer_get_card";
    match peer.get_card().await {
        Ok(_card) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "peer_set_card";
    match peer.set_card(vec!["card item".into()]).await {
        Ok(_card) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "peer_get_card_with_target";
    match peer.get_card_with_target("smoke-peer-2").await {
        Ok(_card) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "peer_set_card_with_target";
    match peer
        .set_card_with_target(vec!["target card item".into()], "smoke-peer-2")
        .await
    {
        Ok(_card) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "peer_message_builder";
    let mut msg_meta = HashMap::new();
    msg_meta.insert("source".into(), json!("smoke-test"));
    match peer.message("hello").metadata(msg_meta.clone()).build() {
        Ok(msg) => {
            if msg.content == "hello"
                && msg.metadata.as_ref().and_then(|m| m.get("source")) == Some(&json!("smoke-test"))
            {
                report.pass(name);
            } else {
                report.fail(name, "message fields mismatch");
            }
        }
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "peer_conclusions";
    let scope = peer.conclusions();
    if scope.observer_id() == "smoke-peer-1" && scope.observed_id() == "smoke-peer-1" {
        report.pass(name);
    } else {
        report.fail(name, "observer_id/observed_id mismatch");
    }

    let name = "peer_conclusions_of";
    let scope = peer.conclusions_of("other");
    if scope.observer_id() == "smoke-peer-1" && scope.observed_id() == "other" {
        report.pass(name);
    } else {
        report.fail(
            name,
            &format!(
                "expected observer=smoke-peer-1 observed=other, got observer={} observed={}",
                scope.observer_id(),
                scope.observed_id(),
            ),
        );
    }

    let name = "peer_representation";
    match peer.representation().await {
        Ok(_rep) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "peer_representation_builder";
    match peer
        .representation_builder()
        .search_query("test")
        .max_conclusions(5)
        .send()
        .await
    {
        Ok(_rep) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "peer_context";
    match peer.context().await {
        Ok(_ctx) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "peer_context_builder";
    match peer
        .context_builder()
        .target("smoke-peer-2")
        .max_conclusions(10)
        .send()
        .await
    {
        Ok(_ctx) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }
}
