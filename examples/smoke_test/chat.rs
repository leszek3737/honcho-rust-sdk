use super::harness::TestReport;
use futures_util::StreamExt;
use honcho_ai::Honcho;
use honcho_ai::types::dialectic::{DialecticOptions, ReasoningLevel};
use tokio::time::{Duration, timeout};

const STREAM_TIMEOUT: Duration = Duration::from_secs(30);

pub async fn run(honcho: &Honcho, report: &mut TestReport) -> honcho_ai::error::Result<()> {
    let peer = honcho.peer("chat-peer").build().await?;
    let session = honcho.session("chat-sess").build().await?;
    session.add_peer(peer.id()).await?;
    session
        .add_messages(vec![peer.message("Context: I like Rust").build()?])
        .await?;

    test_chat_basic(&peer, report).await;
    test_chat_with_options(&peer, report).await;
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
        Ok(Some(_content)) => report.pass(name),
        Ok(None) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }
}

async fn test_chat_with_options(peer: &honcho_ai::Peer, report: &TestReport) {
    let name = "chat_with_options";
    let opts = DialecticOptions::builder()
        .query("Hello")
        .session_id("chat-sess")
        .build();
    match peer.chat_with_options(&opts).await {
        Ok(Some(_content)) => report.pass(name),
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
        Ok(Some(_content)) => report.pass(name),
        Ok(None) => report.pass(name),
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
        Ok(Some(_content)) => report.pass(name),
        Ok(None) => report.pass(name),
        Err(e) => report.fail(name, &e.to_string()),
    }
}

async fn test_chat_stream_basic(peer: &honcho_ai::Peer, report: &TestReport) {
    let name = "chat_stream_basic";
    let stream_result = timeout(STREAM_TIMEOUT, async {
        let mut stream = peer.chat_stream("Tell me something").send().await?;
        let mut count = 0usize;
        while let Some(chunk) = stream.next().await {
            let _text = chunk?;
            count += 1;
        }
        Ok::<usize, honcho_ai::error::HonchoError>(count)
    })
    .await;

    match stream_result {
        Ok(Ok(_count)) => report.pass(name),
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
            let _text = chunk?;
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
            let _text = chunk?;
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
            let _text = chunk?;
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
        while let Some(chunk) = stream.next().await {
            let _text = chunk?;
        }
        let content = stream.final_response().content();
        let complete = stream.is_complete();
        Ok::<(String, bool), honcho_ai::error::HonchoError>((content.to_owned(), complete))
    })
    .await;

    match stream_result {
        Ok(Ok((_content, complete))) => {
            if complete {
                report.pass(name);
            } else {
                report.fail(name, "stream not marked complete after draining");
            }
        }
        Ok(Err(e)) => report.fail(name, &e.to_string()),
        Err(_) => report.fail(name, "timed out after 30s"),
    }
}
