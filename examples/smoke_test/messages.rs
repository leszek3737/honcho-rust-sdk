use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::Cursor;
use std::time::Duration;

use chrono::Utc;
use futures_util::StreamExt;
use honcho_ai::types::common::ReasoningConfiguration;
use honcho_ai::types::message::{MessageConfiguration, MessageSearchOptions};
use honcho_ai::{FileSource, Honcho, Message};
use serde_json::Value;

use super::harness::TestReport;

/// Term seeded into the session so the search tests assert on real hits rather
/// than just "the endpoint did not throw".
const SEARCH_TERM: &str = "Hello world";
/// Query token guaranteed to match [`SEARCH_TERM`].
const SEARCH_QUERY: &str = "Hello";

pub async fn run(honcho: &Honcho, report: &TestReport) {
    report.scenario("messages");

    let peer = match honcho.peer("msg-peer").build().await {
        Ok(p) => p,
        Err(e) => {
            report.fail("setup_peer", &e.to_string());
            return;
        }
    };
    let session = match honcho.session("msg-sess").build().await {
        Ok(s) => s,
        Err(e) => {
            report.fail("setup_session", &e.to_string());
            return;
        }
    };
    if let Err(e) = session.add_peer(peer.id()).await {
        report.fail("setup_add_peer", &e.to_string());
        return;
    }
    // Seed a message containing the search term so the search tests can assert
    // on a non-empty result, not merely that the endpoint did not throw.
    match peer.message(SEARCH_TERM).build() {
        Ok(msg) => {
            if let Err(e) = session.add_messages(vec![msg]).await {
                report.fail("setup_seed_search", &e.to_string());
            }
        }
        Err(e) => report.fail("setup_seed_search", &e.to_string()),
    }

    test_message_create_via_peer(&peer, report);
    test_message_create_with_metadata(&peer, report);
    test_message_create_with_configuration(&peer, report);
    test_message_create_with_created_at(&peer, report);
    test_session_add_messages(&peer, &session, report).await;
    test_session_add_batch_messages(&peer, &session, report).await;
    test_session_messages(&session, report).await;
    test_session_messages_page_info(&session, report).await;
    test_session_messages_with_options(&session, report).await;
    test_session_messages_pagination(&session, report).await;
    test_session_messages_into_stream(&session, report).await;
    test_session_get_message(&session, &peer, report).await;
    test_session_update_message(&session, &peer, report).await;
    test_session_search(&session, report).await;
    test_session_search_with_options(&session, report).await;
    test_peer_search(&peer, report).await;
    test_peer_search_with_options(&peer, report).await;
    test_workspace_search(honcho, report).await;
    test_workspace_search_with_limit(honcho, report).await;
    test_upload_file_bytes(&session, &peer, report).await;
    test_upload_file_streamed(&session, &peer, report).await;
    test_upload_file_with_metadata(&session, &peer, report).await;
    test_upload_file_with_configuration(&session, &peer, report).await;
    test_message_accessors(&peer, &session, honcho.workspace_id(), report).await;
    test_message_display(&peer, &session, report).await;
}

fn test_message_create_via_peer(peer: &honcho_ai::Peer, report: &TestReport) {
    match peer.message("Hello world").build() {
        Ok(msg) => {
            if msg.content == "Hello world" && msg.peer_id == peer.id() {
                report.pass("message_create_via_peer");
            } else {
                report.fail("message_create_via_peer", "content or peer_id mismatch");
            }
        }
        Err(e) => report.fail("message_create_via_peer", &e.to_string()),
    }
}

fn test_message_create_with_metadata(peer: &honcho_ai::Peer, report: &TestReport) {
    let meta = HashMap::from([("tag".to_owned(), Value::String("smoke".to_owned()))]);
    match peer.message("tagged").metadata(meta).build() {
        Ok(msg) => {
            let match_meta = msg
                .metadata
                .as_ref()
                .is_some_and(|m| m.get("tag").is_some_and(|v| v == "smoke"));
            if match_meta {
                report.pass("message_create_with_metadata");
            } else {
                report.fail("message_create_with_metadata", "metadata mismatch");
            }
        }
        Err(e) => report.fail("message_create_with_metadata", &e.to_string()),
    }
}

fn test_message_create_with_configuration(peer: &honcho_ai::Peer, report: &TestReport) {
    // `ReasoningConfiguration` / `MessageConfiguration` are `#[non_exhaustive]`
    // with no builder, so a struct-update (FRU) literal won't compile in a
    // downstream crate (E0639). The default-then-assign pattern is the only
    // option here — SDK gap to address with builders in a future PR.
    #[allow(clippy::field_reassign_with_default)]
    let config = {
        let mut reasoning = ReasoningConfiguration::default();
        reasoning.enabled = Some(true);
        let mut config = MessageConfiguration::default();
        config.reasoning = Some(reasoning);
        config
    };
    match peer.message("reasoned").configuration(config).build() {
        Ok(msg) => {
            let has_reasoning = msg
                .configuration
                .as_ref()
                .and_then(|c| c.reasoning.as_ref())
                .and_then(|r| r.enabled)
                .is_some_and(|e| e);
            if has_reasoning {
                report.pass("message_create_with_configuration");
            } else {
                report.fail("message_create_with_configuration", "configuration not set");
            }
        }
        Err(e) => report.fail("message_create_with_configuration", &e.to_string()),
    }
}

fn test_message_create_with_created_at(peer: &honcho_ai::Peer, report: &TestReport) {
    let now = Utc::now();
    match peer.message("timed").created_at(now).build() {
        Ok(msg) => {
            if msg.created_at == Some(now) {
                report.pass("message_create_with_created_at");
            } else {
                report.fail(
                    "message_create_with_created_at",
                    &format!("expected {:?}, got {:?}", Some(now), msg.created_at),
                );
            }
        }
        Err(e) => report.fail("message_create_with_created_at", &e.to_string()),
    }
}

async fn test_session_add_messages(
    peer: &honcho_ai::Peer,
    session: &honcho_ai::Session,
    report: &TestReport,
) {
    let msg = match peer.message("add-me").build() {
        Ok(m) => m,
        Err(e) => {
            report.fail("session_add_messages", &e.to_string());
            return;
        }
    };
    match session.add_messages(vec![msg]).await {
        Ok(msgs) => {
            if msgs.len() == 1 && msgs[0].content() == "add-me" {
                report.pass("session_add_messages");
            } else {
                report.fail("session_add_messages", "returned messages mismatch");
            }
        }
        Err(e) => report.fail("session_add_messages", &e.to_string()),
    }
}

#[allow(clippy::similar_names)]
async fn test_session_add_batch_messages(
    peer: &honcho_ai::Peer,
    session: &honcho_ai::Session,
    report: &TestReport,
) {
    let built: Result<Vec<_>, _> = ["batch-a", "batch-b", "batch-c"]
        .iter()
        .map(|c| peer.message(*c).build())
        .collect();
    let msgs = match built {
        Ok(m) => m,
        Err(e) => {
            report.fail("session_add_batch_messages", &e.to_string());
            return;
        }
    };
    match session.add_messages(msgs).await {
        Ok(msgs) => {
            if msgs.len() == 3 {
                report.pass("session_add_batch_messages");
            } else {
                report.fail(
                    "session_add_batch_messages",
                    &format!("expected 3 messages, got {}", msgs.len()),
                );
            }
        }
        Err(e) => report.fail("session_add_batch_messages", &e.to_string()),
    }
}

async fn test_session_messages(session: &honcho_ai::Session, report: &TestReport) {
    match session.messages().await {
        Ok(page) => {
            if page.items().is_empty() {
                report.fail("session_messages", "page items empty");
            } else {
                report.pass("session_messages");
            }
        }
        Err(e) => report.fail("session_messages", &e.to_string()),
    }
}

async fn test_session_messages_page_info(session: &honcho_ai::Session, report: &TestReport) {
    match session.messages().await {
        Ok(page) => {
            let mut detail = String::new();
            if page.total() < 1 {
                detail.push_str("total < 1; ");
            }
            // `has_next` must be consistent with page/pages bookkeeping.
            let expected_has_next = page.page() < page.pages();
            if page.has_next() != expected_has_next {
                let _ = write!(
                    detail,
                    "has_next={} but page={} pages={}; ",
                    page.has_next(),
                    page.page(),
                    page.pages()
                );
            }
            if detail.is_empty() {
                report.pass("session_messages_page_info");
            } else {
                report.fail("session_messages_page_info", &detail);
            }
        }
        Err(e) => report.fail("session_messages_page_info", &e.to_string()),
    }
}

async fn test_session_messages_with_options(session: &honcho_ai::Session, report: &TestReport) {
    match session.messages_with_options(None, 1, 10, false).await {
        Ok(page) => {
            if page.items().is_empty() {
                report.fail("session_messages_with_options", "page items empty");
            } else {
                report.pass("session_messages_with_options");
            }
        }
        Err(e) => report.fail("session_messages_with_options", &e.to_string()),
    }
}

async fn test_session_messages_pagination(session: &honcho_ai::Session, report: &TestReport) {
    // Force page size 1 so that, with multiple seeded messages, `has_next` is
    // true and `next_page()` is actually exercised.
    match session.messages_with_options(None, 1, 1, false).await {
        Ok(page) => {
            if page.has_next() {
                match page.next_page().await {
                    Ok(Some(_next)) => report.pass("session_messages_pagination"),
                    Ok(None) => report.fail(
                        "session_messages_pagination",
                        "has_next true but no next page",
                    ),
                    Err(e) => report.fail("session_messages_pagination", &e.to_string()),
                }
            } else {
                // size=1 with <=1 message: legitimately the last page.
                report.pass("session_messages_pagination");
            }
        }
        Err(e) => report.fail("session_messages_pagination", &e.to_string()),
    }
}

async fn test_session_messages_into_stream(session: &honcho_ai::Session, report: &TestReport) {
    match session.messages().await {
        Ok(page) => {
            let stream = page.into_stream();
            let all: Vec<_> = stream.collect().await;
            if all.is_empty() {
                report.fail("session_messages_into_stream", "stream yielded nothing");
            } else if all.iter().any(Result::is_err) {
                report.fail(
                    "session_messages_into_stream",
                    "stream yielded one or more errors",
                );
            } else {
                report.pass("session_messages_into_stream");
            }
        }
        Err(e) => report.fail("session_messages_into_stream", &e.to_string()),
    }
}

async fn test_session_get_message(
    session: &honcho_ai::Session,
    peer: &honcho_ai::Peer,
    report: &TestReport,
) {
    let msg = match peer.message("get-me").build() {
        Ok(m) => m,
        Err(e) => {
            report.fail("session_get_message", &e.to_string());
            return;
        }
    };
    let added = match session.add_messages(vec![msg]).await {
        Ok(msgs) => msgs,
        Err(e) => {
            report.fail("session_get_message", &e.to_string());
            return;
        }
    };
    let Some(first) = added.first() else {
        report.fail("session_get_message", "no messages returned from add");
        return;
    };
    let msg_id = first.id();
    match session.get_message(msg_id).await {
        Ok(fetched) => {
            if fetched.id() == msg_id {
                report.pass("session_get_message");
            } else {
                report.fail("session_get_message", "id mismatch");
            }
        }
        Err(e) => report.fail("session_get_message", &e.to_string()),
    }
}

async fn test_session_update_message(
    session: &honcho_ai::Session,
    peer: &honcho_ai::Peer,
    report: &TestReport,
) {
    let msg = match peer.message("update-me").build() {
        Ok(m) => m,
        Err(e) => {
            report.fail("session_update_message", &e.to_string());
            return;
        }
    };
    let added = match session.add_messages(vec![msg]).await {
        Ok(msgs) => msgs,
        Err(e) => {
            report.fail("session_update_message", &e.to_string());
            return;
        }
    };
    let Some(first) = added.first() else {
        report.fail("session_update_message", "no messages returned from add");
        return;
    };
    let msg_id = first.id();
    let update_meta = HashMap::from([("updated".to_owned(), Value::Bool(true))]);
    match session.update_message(msg_id, update_meta).await {
        Ok(updated) => {
            let has_updated = updated
                .metadata()
                .get("updated")
                .is_some_and(|v| v == &Value::Bool(true));
            if has_updated {
                report.pass("session_update_message");
            } else {
                report.fail("session_update_message", "metadata not updated");
            }
        }
        Err(e) => report.fail("session_update_message", &e.to_string()),
    }
}

/// Run a search closure with retry to ride out async message indexing, then
/// assert the result is non-empty.
async fn assert_search_nonempty<F, Fut>(name: &str, report: &TestReport, mut search: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = honcho_ai::error::Result<Vec<Message>>>,
{
    let mut last_err: Option<String> = None;
    for attempt in 0..5 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(500 * attempt)).await;
        }
        match search().await {
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

async fn test_session_search(session: &honcho_ai::Session, report: &TestReport) {
    assert_search_nonempty("session_search", report, || session.search(SEARCH_QUERY)).await;
}

async fn test_session_search_with_options(session: &honcho_ai::Session, report: &TestReport) {
    let opts = MessageSearchOptions::builder()
        .query(SEARCH_QUERY)
        .limit(5)
        .build();
    assert_search_nonempty("session_search_with_options", report, || {
        session.search_with_options(&opts)
    })
    .await;
}

async fn test_peer_search(peer: &honcho_ai::Peer, report: &TestReport) {
    assert_search_nonempty("peer_search", report, || peer.search(SEARCH_QUERY)).await;
}

async fn test_peer_search_with_options(peer: &honcho_ai::Peer, report: &TestReport) {
    let opts = MessageSearchOptions::builder()
        .query(SEARCH_QUERY)
        .limit(5)
        .build();
    assert_search_nonempty("peer_search_with_options", report, || {
        peer.search_with_options(&opts)
    })
    .await;
}

async fn test_workspace_search(honcho: &Honcho, report: &TestReport) {
    assert_search_nonempty("workspace_search", report, || {
        honcho.search(SEARCH_QUERY).build()
    })
    .await;
}

async fn test_workspace_search_with_limit(honcho: &Honcho, report: &TestReport) {
    assert_search_nonempty("workspace_search_with_limit", report, || {
        honcho.search(SEARCH_QUERY).limit(5).build()
    })
    .await;
}

async fn test_upload_file_bytes(
    session: &honcho_ai::Session,
    peer: &honcho_ai::Peer,
    report: &TestReport,
) {
    let source = FileSource::bytes("test.txt", b"content", "text/plain");
    match session.upload_file(source).peer(peer.id()).send().await {
        Ok(msgs) => {
            if msgs.is_empty() {
                report.fail("upload_file_bytes", "no messages returned");
            } else {
                report.pass("upload_file_bytes");
            }
        }
        Err(e) => report.fail("upload_file_bytes", &e.to_string()),
    }
}

async fn test_upload_file_streamed(
    session: &honcho_ai::Session,
    peer: &honcho_ai::Peer,
    report: &TestReport,
) {
    let cursor = Cursor::new(b"stream content".to_vec());
    match session
        .upload_file_streamed("test.txt", cursor, "text/plain")
        .peer(peer.id())
        .send()
        .await
    {
        Ok(msgs) => {
            if msgs.is_empty() {
                report.fail("upload_file_streamed", "no messages returned");
            } else {
                report.pass("upload_file_streamed");
            }
        }
        Err(e) => report.fail("upload_file_streamed", &e.to_string()),
    }
}

async fn test_upload_file_with_metadata(
    session: &honcho_ai::Session,
    peer: &honcho_ai::Peer,
    report: &TestReport,
) {
    let source = FileSource::bytes("meta.txt", b"meta content", "text/plain");
    let meta = serde_json::json!({"source": "upload"});
    match session
        .upload_file(source)
        .peer(peer.id())
        .metadata(meta)
        .send()
        .await
    {
        Ok(msgs) => {
            if msgs.is_empty() {
                report.fail("upload_file_with_metadata", "no messages returned");
            } else {
                report.pass("upload_file_with_metadata");
            }
        }
        Err(e) => report.fail("upload_file_with_metadata", &e.to_string()),
    }
}

async fn test_upload_file_with_configuration(
    session: &honcho_ai::Session,
    peer: &honcho_ai::Peer,
    report: &TestReport,
) {
    let source = FileSource::bytes("cfg.txt", b"cfg content", "text/plain");
    let config = serde_json::json!({"reasoning": {"enabled": true}});
    match session
        .upload_file(source)
        .peer(peer.id())
        .configuration(config)
        .send()
        .await
    {
        Ok(msgs) => {
            if msgs.is_empty() {
                report.fail("upload_file_with_configuration", "no messages returned");
            } else {
                report.pass("upload_file_with_configuration");
            }
        }
        Err(e) => report.fail("upload_file_with_configuration", &e.to_string()),
    }
}

async fn test_message_accessors(
    peer: &honcho_ai::Peer,
    session: &honcho_ai::Session,
    workspace_id: &str,
    report: &TestReport,
) {
    let msg = match peer.message("accessor-test").build() {
        Ok(m) => m,
        Err(e) => {
            report.fail("message_accessors", &e.to_string());
            return;
        }
    };
    let added = match session.add_messages(vec![msg]).await {
        Ok(msgs) => msgs,
        Err(e) => {
            report.fail("message_accessors", &e.to_string());
            return;
        }
    };
    let Some(m) = added.first() else {
        report.fail("message_accessors", "no messages returned");
        return;
    };
    let mut detail = String::new();
    if m.id().is_empty() {
        detail.push_str("id empty; ");
    }
    if m.content() != "accessor-test" {
        detail.push_str("content mismatch; ");
    }
    if m.peer_id() != peer.id() {
        detail.push_str("peer_id mismatch; ");
    }
    if m.session_id() != session.id() {
        detail.push_str("session_id mismatch; ");
    }
    if m.created_at().timestamp() == 0 {
        detail.push_str("created_at zero; ");
    }
    if m.workspace_id() != workspace_id {
        let _ = write!(
            detail,
            "workspace_id mismatch: expected {workspace_id}, got {}; ",
            m.workspace_id()
        );
    }
    // Just exercise the remaining accessors (no strong invariant available).
    let _ = m.metadata();
    let _ = m.token_count();
    if detail.is_empty() {
        report.pass("message_accessors");
    } else {
        report.fail("message_accessors", &detail);
    }
}

async fn test_message_display(
    peer: &honcho_ai::Peer,
    session: &honcho_ai::Session,
    report: &TestReport,
) {
    let msg = match peer.message("display me").build() {
        Ok(m) => m,
        Err(e) => {
            report.fail("message_display", &e.to_string());
            return;
        }
    };
    let added = match session.add_messages(vec![msg]).await {
        Ok(msgs) => msgs,
        Err(e) => {
            report.fail("message_display", &e.to_string());
            return;
        }
    };
    let Some(first) = added.first() else {
        report.fail("message_display", "no messages returned");
        return;
    };
    let displayed = first.to_string();
    if displayed.contains("display me") {
        report.pass("message_display");
    } else {
        report.fail("message_display", &format!("Display output: {displayed}"));
    }
}
