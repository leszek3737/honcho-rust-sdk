#![allow(clippy::print_stderr)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

use std::time::Duration;

use futures_util::StreamExt;
use honcho_ai::ReasoningLevel;
use honcho_ai::error::HonchoError;

use crate::common::{WorkspaceGuard, try_client};

/// Upper bound for a single streaming chat to drain. A live dialectic response
/// should finish well within this; exceeding it means the stream hung and the
/// test must fail rather than silently pass on a partial buffer.
const STREAM_TIMEOUT: Duration = Duration::from_secs(30);

#[tokio::test(flavor = "multi_thread")]
async fn chat_stream_drains_content() {
    let Some(client) = try_client().await else {
        return;
    };
    // RAII guard: workspace teardown runs on Drop (even on assert unwind),
    // so no orphaned workspace is left behind on failure.
    let guard = WorkspaceGuard::new(client);
    let client = guard.client();

    let peer = client.peer("stream-test-peer").build().await.unwrap();

    let mut stream = match peer.chat_stream("Hi").send().await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("skipping stream test: could not start stream: {e}");
            return;
        }
    };

    let mut collected = String::new();
    let mut chunk_count = 0usize;
    let mut stream_errored = false;

    // `DialecticStream` is `Unpin`, so `StreamExt::next` works without `Box::pin`.
    let timeout = tokio::time::timeout(STREAM_TIMEOUT, async {
        while let Some(result) = stream.next().await {
            match result {
                Ok(chunk) => {
                    collected.push_str(&chunk);
                    chunk_count += 1;
                }
                Err(e) => {
                    eprintln!("stream error: {e}");
                    stream_errored = true;
                    break;
                }
            }
        }
    })
    .await;

    // A hang must fail the test: the timeout result is asserted, not discarded.
    assert!(
        timeout.is_ok(),
        "stream did not finish within {STREAM_TIMEOUT:?}"
    );
    // A mid-stream transport/SSE error must fail the test, not be swallowed.
    assert!(!stream_errored, "stream yielded an error mid-drain");
    assert!(
        stream.is_complete(),
        "stream did not reach a clean end-of-stream"
    );
    assert!(!stream.is_errored(), "stream terminated with an error");
    assert!(
        chunk_count > 0,
        "expected at least one chunk from stream, got {chunk_count}"
    );

    // The accumulated `final_response` must be non-empty and must exactly match
    // the chunks we drained (pass-through accumulation parity).
    let final_text = stream.final_response().content().to_owned();
    assert!(!final_text.is_empty(), "final_response was empty");
    assert_eq!(
        collected, final_text,
        "drained chunks must equal the accumulated final_response"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn chat_stream_honors_builder_options() {
    let Some(client) = try_client().await else {
        return;
    };
    let guard = WorkspaceGuard::new(client);
    let client = guard.client();

    let peer = client.peer("stream-opts-peer").build().await.unwrap();
    let session = client.session("stream-opts-session").build().await.unwrap();
    session.add_peer("stream-opts-peer").await.unwrap();

    let mut stream = match peer
        .chat_stream("Summarize what you know")
        .target("stream-opts-peer")
        .session(session.id())
        .reasoning_level(ReasoningLevel::High)
        .send()
        .await
    {
        Ok(s) => s,
        Err(e) => {
            eprintln!("skipping options stream test: could not start stream: {e}");
            return;
        }
    };

    let mut stream_errored = false;
    let timeout = tokio::time::timeout(STREAM_TIMEOUT, async {
        while let Some(result) = stream.next().await {
            if result.is_err() {
                stream_errored = true;
                break;
            }
        }
    })
    .await;

    assert!(
        timeout.is_ok(),
        "options stream did not finish within {STREAM_TIMEOUT:?}"
    );
    assert!(!stream_errored, "options stream yielded an error mid-drain");
    assert!(stream.is_complete(), "options stream did not complete");
    assert!(
        !stream.is_errored(),
        "options stream terminated with an error"
    );

    session.delete().await.ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn chat_stream_rejects_invalid_query() {
    let Some(client) = try_client().await else {
        return;
    };
    let guard = WorkspaceGuard::new(client);
    let client = guard.client();

    let peer = client.peer("stream-validation-peer").build().await.unwrap();

    // Empty query is rejected client-side before any transport happens.
    let empty_err = peer.chat_stream("").send().await.unwrap_err();
    assert!(
        matches!(empty_err, HonchoError::Validation(_)),
        "expected Validation for empty query, got: {empty_err:?}"
    );

    // Over-long query (> 10_000 chars) is rejected client-side as well.
    let long_query = "a".repeat(10_001);
    let long_err = peer.chat_stream(long_query).send().await.unwrap_err();
    assert!(
        matches!(long_err, HonchoError::Validation(_)),
        "expected Validation for over-long query, got: {long_err:?}"
    );
}
