use super::harness::TestReport;
use futures_util::StreamExt;
use honcho_ai::Honcho;
use honcho_ai::types::dialectic::{DialecticOptions, ReasoningLevel};
use tokio::time::{Duration, timeout};

const STREAM_TIMEOUT: Duration = Duration::from_secs(30);

pub async fn run(honcho: &Honcho, report: &TestReport) -> honcho_ai::error::Result<()> {
    report.scenario("chat");

    // Setup failures are reported here via `report.fail` and the scenario
    // returns `Ok(())`; main does NOT additionally treat it as an abort, so a
    // setup failure is counted exactly once (and is RED).
    let peer = match honcho.peer("chat-peer").build().await {
        Ok(p) => p,
        Err(e) => {
            report.fail("chat setup", &e.to_string());
            return Ok(());
        }
    };
    let session = match honcho.session("chat-sess").build().await {
        Ok(s) => s,
        Err(e) => {
            report.fail("chat setup", &e.to_string());
            return Ok(());
        }
    };
    if let Err(e) = session.add_peer(peer.id()).await {
        report.fail("chat setup", &e.to_string());
        return Ok(());
    }
    let seed = match peer.message("Context: I like Rust").build() {
        Ok(m) => m,
        Err(e) => {
            report.fail("chat setup", &e.to_string());
            return Ok(());
        }
    };
    if let Err(e) = session.add_messages(vec![seed]).await {
        report.fail("chat setup", &e.to_string());
        return Ok(());
    }

    test_chat_basic(&peer, report).await;
    test_chat_with_options(&peer, &session, report).await;
    test_chat_with_streaming_false(&peer, report).await;
    test_chat_with_reasoning_level(&peer, report).await;
    test_chat_stream_basic(&peer, report).await;
    test_chat_stream_with_target(&peer, report).await;
    test_chat_stream_with_session(&peer, &session, report).await;
    test_chat_stream_with_reasoning(&peer, report).await;
    test_dialectic_stream_final_response(&peer, report).await;

    Ok(())
}

async fn test_chat_basic(peer: &honcho_ai::Peer, report: &TestReport) {
    let name = "chat_basic";
    match peer.chat("What do you know?").await {
        // The dialectic deriver is async: a brand-new session may legitimately
        // return `None`. But when content *is* produced it must be non-empty —
        // an "always empty string" regression must not pass.
        Ok(Some(content)) => {
            if content.trim().is_empty() {
                report.fail(name, "Some(content) but content is empty");
            } else {
                report.pass(name);
            }
        }
        Ok(None) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }
}

async fn test_chat_with_options(
    peer: &honcho_ai::Peer,
    session: &honcho_ai::Session,
    report: &TestReport,
) {
    let name = "chat_with_options";
    let opts = DialecticOptions::builder()
        .query("Hello")
        // Scope to the real session id, not a duplicated literal.
        .session_id(session.id())
        .build();
    match peer.chat_with_options(&opts).await {
        Ok(Some(content)) => {
            if content.trim().is_empty() {
                report.fail(name, "Some(content) but content is empty");
            } else {
                report.pass(name);
            }
        }
        Ok(None) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }
}

async fn test_chat_with_streaming_false(peer: &honcho_ai::Peer, report: &TestReport) {
    let name = "chat_with_streaming_false";
    let opts = DialecticOptions::builder()
        .query("Hello with stream false")
        .stream(false)
        .build();
    match peer.chat_with_options(&opts).await {
        Ok(_content) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }
}

async fn test_chat_with_reasoning_level(peer: &honcho_ai::Peer, report: &TestReport) {
    let name = "chat_with_reasoning_level";
    let opts = DialecticOptions::builder()
        .query("Think harder")
        .reasoning_level(ReasoningLevel::Medium)
        .build();
    match peer.chat_with_options(&opts).await {
        Ok(_content) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }
}

async fn test_chat_stream_basic(peer: &honcho_ai::Peer, report: &TestReport) {
    let name = "chat_stream_basic";
    let stream_result = timeout(STREAM_TIMEOUT, async {
        let mut stream = peer.chat_stream("Tell me something").send().await?;
        let mut count = 0;
        while let Some(chunk) = stream.next().await {
            chunk?;
            count += 1;
        }
        Ok::<usize, honcho_ai::error::HonchoError>(count)
    })
    .await;

    match stream_result {
        // A stream that yields no chunks at all is a regression for the basic
        // streaming path — assert at least one chunk arrived.
        Ok(Ok(count)) => {
            if count == 0 {
                report.fail(name, "stream yielded zero chunks");
            } else {
                report.pass(name);
            }
        }
        Ok(Err(e)) => report.fail(name, &e.to_string()),
        Err(_) => report.fail(name, "timed out after 30s"),
    }
}

async fn test_chat_stream_with_target(peer: &honcho_ai::Peer, report: &TestReport) {
    let name = "chat_stream_with_target";
    let stream_result = timeout(STREAM_TIMEOUT, async {
        let mut stream = peer
            .chat_stream("Hello target")
            .target("other-peer")
            .send()
            .await?;
        while let Some(chunk) = stream.next().await {
            chunk?;
        }
        Ok::<(), honcho_ai::error::HonchoError>(())
    })
    .await;

    match stream_result {
        Ok(Ok(())) => report.pass(name),
        Ok(Err(e)) => report.fail(name, &e.to_string()),
        Err(_) => report.fail(name, "timed out after 30s"),
    }
}

async fn test_chat_stream_with_session(
    peer: &honcho_ai::Peer,
    session: &honcho_ai::Session,
    report: &TestReport,
) {
    let name = "chat_stream_with_session";
    let stream_result = timeout(STREAM_TIMEOUT, async {
        let mut stream = peer
            .chat_stream("Session-scoped query")
            .session(session.id())
            .send()
            .await?;
        while let Some(chunk) = stream.next().await {
            chunk?;
        }
        Ok::<(), honcho_ai::error::HonchoError>(())
    })
    .await;

    match stream_result {
        Ok(Ok(())) => report.pass(name),
        Ok(Err(e)) => report.fail(name, &e.to_string()),
        Err(_) => report.fail(name, "timed out after 30s"),
    }
}

async fn test_chat_stream_with_reasoning(peer: &honcho_ai::Peer, report: &TestReport) {
    let name = "chat_stream_with_reasoning";
    let stream_result = timeout(STREAM_TIMEOUT, async {
        let mut stream = peer
            .chat_stream("Deep thought")
            .reasoning_level(ReasoningLevel::High)
            .send()
            .await?;
        while let Some(chunk) = stream.next().await {
            chunk?;
        }
        Ok::<(), honcho_ai::error::HonchoError>(())
    })
    .await;

    match stream_result {
        Ok(Ok(())) => report.pass(name),
        Ok(Err(e)) => report.fail(name, &e.to_string()),
        Err(_) => report.fail(name, "timed out after 30s"),
    }
}

async fn test_dialectic_stream_final_response(peer: &honcho_ai::Peer, report: &TestReport) {
    let name = "dialectic_stream_final_response";
    let stream_result = timeout(STREAM_TIMEOUT, async {
        let mut stream = peer.chat_stream("Accumulate me").send().await?;
        let mut accumulated = String::new();
        while let Some(chunk) = stream.next().await {
            accumulated.push_str(&chunk?);
        }
        let final_content = stream.final_response().content().to_owned();
        let complete = stream.is_complete();
        Ok::<(String, String, bool), honcho_ai::error::HonchoError>((
            accumulated,
            final_content,
            complete,
        ))
    })
    .await;

    match stream_result {
        Ok(Ok((accumulated, final_content, complete))) => {
            if !complete {
                report.fail(name, "stream not marked complete after draining");
            } else if final_content != accumulated {
                report.fail(
                    name,
                    &format!(
                        "final_response ({final_content:?}) != accumulated chunks ({accumulated:?})"
                    ),
                );
            } else {
                report.pass(name);
            }
        }
        Ok(Err(e)) => report.fail(name, &e.to_string()),
        Err(_) => report.fail(name, "timed out after 30s"),
    }
}
