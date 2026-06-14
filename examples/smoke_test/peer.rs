use super::harness::TestReport;
use honcho_ai::Honcho;
use honcho_ai::types::message::MessageSearchOptions;
use honcho_ai::types::peer::PeerConfig;
use honcho_ai::types::session::SessionListOptions;
use serde_json::json;
use std::collections::HashMap;
use std::time::Duration;

/// Report `Ok` as pass and `Err` as fail under `name` — collapses the dozens of
/// identical `match { Ok => pass, Err => fail }` blocks in this scenario.
fn check<T>(name: &str, result: honcho_ai::error::Result<T>, report: &TestReport) {
    match result {
        Ok(_) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }
}

#[allow(clippy::too_many_lines)]
pub async fn run(honcho: &Honcho, report: &TestReport) {
    report.scenario("peer");

    let name = "peer_create";
    let peer = match honcho.peer("smoke-peer-1").build().await {
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
    let metadata_map = HashMap::from([("role".to_owned(), json!("tester"))]);
    check(
        name,
        honcho
            .peer("smoke-peer-2")
            .metadata(metadata_map)
            .build()
            .await,
        report,
    );

    // Seed a message so the search tests below assert on a real hit instead of
    // an always-empty result.
    let search_session = match honcho.session("peer-search-sess").build().await {
        Ok(s) => s,
        Err(e) => {
            report.fail("peer_search_setup", &e.to_string());
            return;
        }
    };
    if let Err(e) = search_session.add_peer(peer.id()).await {
        report.fail("peer_search_setup", &e.to_string());
        return;
    }
    match peer.message("Hello world").build() {
        Ok(msg) => {
            if let Err(e) = search_session.add_messages(vec![msg]).await {
                report.fail("peer_search_setup", &e.to_string());
            }
        }
        Err(e) => report.fail("peer_search_setup", &e.to_string()),
    }

    let name = "peer_get_metadata";
    check(name, peer.get_metadata().await, report);

    let name = "peer_set_metadata";
    let meta = HashMap::from([("updated".to_owned(), json!(true))]);
    match peer.set_metadata(meta).await {
        Ok(()) => match peer.get_metadata().await {
            Ok(got) => {
                if got.get("updated") == Some(&json!(true)) {
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
    check(name, peer.get_configuration().await, report);

    let name = "peer_set_configuration";
    #[allow(clippy::field_reassign_with_default)]
    let config = {
        // `PeerConfig` is `#[non_exhaustive]` without a builder (SDK gap), so a
        // default-then-assign is the only way to construct it downstream.
        let mut config = PeerConfig::default();
        config.observe_me = Some(true);
        config.observe_others = Some(false);
        config
    };
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
    check(name, peer.get_configuration_raw().await, report);

    let name = "peer_set_configuration_raw";
    let raw_cfg = HashMap::from([
        ("observe_me".to_owned(), json!(false)),
        ("observe_others".to_owned(), json!(true)),
    ]);
    match peer.set_configuration_raw(raw_cfg).await {
        Ok(()) => match peer.get_configuration_raw().await {
            Ok(got) => {
                // Verify both fields, not just one — a partial write must fail.
                if got.get("observe_me") == Some(&json!(false))
                    && got.get("observe_others") == Some(&json!(true))
                {
                    report.pass(name);
                } else {
                    report.fail(
                        name,
                        &format!("raw configuration mismatch after set: {got:?}"),
                    );
                }
            }
            Err(e) => report.fail(name, &format!("verify raw get failed: {e}")),
        },
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "peer_update";
    let new_meta = HashMap::from([("patched".to_owned(), json!("yes"))]);
    match peer.update(new_meta).await {
        Ok(()) => match peer.get_metadata().await {
            // `update` is a PUT (replace). Verify the patched key survived; an
            // Ok-only assertion would miss a write that silently dropped it.
            Ok(got) if got.get("patched") == Some(&json!("yes")) => report.pass(name),
            Ok(got) => report.fail(name, &format!("patched key missing after update: {got:?}")),
            Err(e) => report.fail(name, &format!("verify get failed: {e}")),
        },
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "peer_refresh";
    check(name, peer.refresh().await, report);

    let name = "peer_sessions";
    check(name, peer.sessions().await, report);

    let name = "peer_sessions_with_options";
    let opts = SessionListOptions::builder().page(1).size(10).build();
    check(name, peer.sessions_with_options(&opts).await, report);

    test_peer_search(&peer, report).await;

    let name = "peer_search_with_options";
    let search_opts = MessageSearchOptions::builder()
        .query("Hello")
        .limit(5)
        .build();
    check(name, peer.search_with_options(&search_opts).await, report);

    let name = "peer_get_card";
    check(name, peer.get_card().await, report);

    // set_card → get_card round-trip: a set that does not stick must fail.
    let name = "peer_set_card";
    match peer.set_card(vec!["card item".to_owned()]).await {
        Ok(_set) => match peer.get_card().await {
            Ok(Some(card)) if card.iter().any(|s| s == "card item") => report.pass(name),
            Ok(card) => report.fail(name, &format!("card not persisted after set: {card:?}")),
            Err(e) => report.fail(name, &format!("verify get_card failed: {e}")),
        },
        Err(e) => report.fail(name, &e.to_string()),
    }

    let name = "peer_get_card_with_target";
    check(
        name,
        peer.get_card_with_target("smoke-peer-2").await,
        report,
    );

    let name = "peer_set_card_with_target";
    check(
        name,
        peer.set_card_with_target(vec!["target card item".to_owned()], "smoke-peer-2")
            .await,
        report,
    );

    let name = "peer_message_builder";
    let msg_meta = HashMap::from([("source".to_owned(), json!("smoke-test"))]);
    match peer.message("hello").metadata(msg_meta).build() {
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
    check(name, peer.representation().await, report);

    let name = "peer_representation_builder";
    check(
        name,
        peer.representation_builder()
            .search_query("test")
            .max_conclusions(5)
            .send()
            .await,
        report,
    );

    let name = "peer_context";
    check(name, peer.context().await, report);

    let name = "peer_context_builder";
    check(
        name,
        peer.context_builder()
            .target("smoke-peer-2")
            .max_conclusions(10)
            .send()
            .await,
        report,
    );
}

async fn test_peer_search(peer: &honcho_ai::Peer, report: &TestReport) {
    let name = "peer_search";
    let mut last_err: Option<String> = None;
    for attempt in 0..5 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(500 * attempt)).await;
        }
        match peer.search("Hello").await {
            Ok(results) if !results.is_empty() => {
                report.pass(name);
                return;
            }
            Ok(_) => {}
            Err(e) => last_err = Some(e.to_string()),
        }
    }
    match last_err {
        Some(e) => report.fail(name, &format!("persistent error: {e}")),
        None => report.fail(name, "no results after retries (indexing may be slow)"),
    }
}
