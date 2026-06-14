use std::collections::HashMap;
use std::io::Cursor;

use chrono::Utc;
use futures_util::StreamExt;
use honcho_ai::types::common::ReasoningConfiguration;
use honcho_ai::types::message::{MessageConfiguration, MessageSearchOptions};
use honcho_ai::{FileSource, Honcho};
use serde_json::Value;

use super::harness::TestReport;

pub async fn run(honcho: &Honcho, report: &mut TestReport) {
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
    let mut meta = HashMap::new();
    meta.insert("tag".to_owned(), Value::String("smoke".to_owned()));
    match peer.message("tagged").metadata(meta.clone()).build() {
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
    let mut reasoning = ReasoningConfiguration::default();
    reasoning.enabled = Some(true);
    let mut config = MessageConfiguration::default();
    config.reasoning = Some(reasoning);
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
            if msg.created_at.is_some() {
                report.pass("message_create_with_created_at");
            } else {
                report.fail("message_create_with_created_at", "created_at not set");
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
    let msg1 = match peer.message("batch-a").build() {
        Ok(m) => m,
        Err(e) => {
            report.fail("session_add_batch_messages", &e.to_string());
            return;
        }
    };
    let msg2 = match peer.message("batch-b").build() {
        Ok(m) => m,
        Err(e) => {
            report.fail("session_add_batch_messages", &e.to_string());
            return;
        }
    };
    let msg3 = match peer.message("batch-c").build() {
        Ok(m) => m,
        Err(e) => {
            report.fail("session_add_batch_messages", &e.to_string());
            return;
        }
    };
    match session.add_messages(vec![msg1, msg2, msg3]).await {
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
            let _total = page.total();
            let _page_num = page.page();
            let _size = page.size();
            let _pages = page.pages();
            let _has_next = page.has_next();
            report.pass("session_messages_page_info");
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
    match session.messages().await {
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
            let all: Vec<_> = stream.collect::<Vec<_>>().await;
            if all.is_empty() {
                report.fail("session_messages_into_stream", "stream yielded nothing");
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
    if added.is_empty() {
        report.fail("session_get_message", "no messages returned from add");
        return;
    }
    let msg_id = added[0].id();
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
    if added.is_empty() {
        report.fail("session_update_message", "no messages returned from add");
        return;
    }
    let msg_id = added[0].id();
    let mut update_meta = HashMap::new();
    update_meta.insert("updated".to_owned(), Value::Bool(true));
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

async fn test_session_search(session: &honcho_ai::Session, report: &TestReport) {
    match session.search("Hello").await {
        Ok(results) => {
            let _ = results;
            report.pass("session_search");
        }
        Err(e) => report.fail("session_search", &e.to_string()),
    }
}

async fn test_session_search_with_options(session: &honcho_ai::Session, report: &TestReport) {
    let opts = MessageSearchOptions::builder()
        .query("Hello")
        .limit(5)
        .build();
    match session.search_with_options(&opts).await {
        Ok(results) => {
            let _ = results;
            report.pass("session_search_with_options");
        }
        Err(e) => report.fail("session_search_with_options", &e.to_string()),
    }
}

async fn test_peer_search(peer: &honcho_ai::Peer, report: &TestReport) {
    match peer.search("Hello").await {
        Ok(results) => {
            let _ = results;
            report.pass("peer_search");
        }
        Err(e) => report.fail("peer_search", &e.to_string()),
    }
}

async fn test_peer_search_with_options(peer: &honcho_ai::Peer, report: &TestReport) {
    let opts = MessageSearchOptions::builder()
        .query("Hello")
        .limit(5)
        .build();
    match peer.search_with_options(&opts).await {
        Ok(results) => {
            let _ = results;
            report.pass("peer_search_with_options");
        }
        Err(e) => report.fail("peer_search_with_options", &e.to_string()),
    }
}

async fn test_workspace_search(honcho: &Honcho, report: &TestReport) {
    match honcho.search("Hello").build().await {
        Ok(results) => {
            let _ = results;
            report.pass("workspace_search");
        }
        Err(e) => report.fail("workspace_search", &e.to_string()),
    }
}

async fn test_workspace_search_with_limit(honcho: &Honcho, report: &TestReport) {
    match honcho.search("Hello").limit(5).build().await {
        Ok(results) => {
            let _ = results;
            report.pass("workspace_search_with_limit");
        }
        Err(e) => report.fail("workspace_search_with_limit", &e.to_string()),
    }
}

async fn test_upload_file_bytes(
    session: &honcho_ai::Session,
    peer: &honcho_ai::Peer,
    report: &TestReport,
) {
    let source = FileSource::bytes("test.txt", b"content".as_slice(), "text/plain");
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
    let source = FileSource::bytes("meta.txt", b"meta content".as_slice(), "text/plain");
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
    let source = FileSource::bytes("cfg.txt", b"cfg content".as_slice(), "text/plain");
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
    if added.is_empty() {
        report.fail("message_accessors", "no messages returned");
        return;
    }
    let m = &added[0];
    let _ = m.id();
    let _ = m.content();
    let _ = m.peer_id();
    let _ = m.session_id();
    let _ = m.metadata();
    let _ = m.created_at();
    let _ = m.token_count();
    let ws = m.workspace_id();
    if ws == workspace_id {
        report.pass("message_accessors");
    } else {
        report.fail(
            "message_accessors",
            &format!("workspace_id mismatch: expected {workspace_id}, got {ws}"),
        );
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
        Ok(msgs) if !msgs.is_empty() => msgs,
        Ok(_) => {
            report.fail("message_display", "no messages returned");
            return;
        }
        Err(e) => {
            report.fail("message_display", &e.to_string());
            return;
        }
    };
    let displayed = format!("{}", added[0]);
    if displayed.contains("display me") {
        report.pass("message_display");
    } else {
        report.fail("message_display", &format!("Display output: {displayed}"));
    }
}
