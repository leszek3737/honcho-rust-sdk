//! Cancellation / teardown behaviour for the SSE-style chat stream.
//!
//! These tests exercise the cancellation contract of [`DialecticStream`] (the
//! wrapper used by `chat_stream`) and the real TCP teardown that happens when a
//! streaming HTTP body is dropped mid-read. They use in-memory streams and a
//! local socket only — no live Honcho server.
//!
//! Timing is virtual: the cancellation tests run under `start_paused = true`
//! and rely on tokio's auto-advance, never on wall-clock sleeps. The one test
//! that needs a real socket synchronises with a oneshot, not a sleep.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

mod common;

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures_util::{Stream, StreamExt};
use honcho_ai::dialectic_stream::DialecticStream;
use honcho_ai::error::HonchoError;

/// A long virtual delay that never elapses within any test's time budget; used
/// to keep the inner generator parked so we can observe its teardown.
const NEVER: Duration = Duration::from_secs(300);

/// Boxed inner chunk stream — the same `Unpin` shape `DialecticStream` wraps
/// around the real SSE byte stream.
type BoxedChunks = Pin<Box<dyn Stream<Item = honcho_ai::error::Result<String>> + Send>>;

/// Builds an in-memory `DialecticStream` whose first chunk is ready immediately
/// and whose second chunk is gated behind a `NEVER` sleep. When `on_park` is
/// `Some`, its `Drop` guard becomes live only after the first chunk is consumed
/// and the generator is resumed into the sleep — so dropping the stream there
/// proves the in-flight inner future is torn down.
fn slow_stream(on_park: Option<DropGuard>) -> DialecticStream<BoxedChunks> {
    let inner: BoxedChunks = Box::pin(async_stream::stream! {
        yield Ok::<_, HonchoError>("first".to_string());
        // `_guard` is constructed here, i.e. only on the SECOND poll. It then
        // stays live across the never-elapsing sleep, so a drop at that suspend
        // point runs its `Drop`.
        let _guard = on_park;
        tokio::time::sleep(NEVER).await;
        yield Ok::<_, HonchoError>("second".to_string());
    });
    DialecticStream::new(inner)
}

/// Flips an `AtomicBool` on `Drop`. Sentinel for "the inner future was actually
/// cancelled", as opposed to merely "the outer poll returned".
struct DropGuard(Arc<AtomicBool>);

impl Drop for DropGuard {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// A `select!` racing a slow second poll against a short timeout must cancel the
/// pending `next()` and take the timeout arm.
///
/// The first chunk is ready immediately, so the cancellation only bites on the
/// *second* poll, which parks on `NEVER`. Under `start_paused`, tokio
/// auto-advances virtual time to the earliest timer (the 1s timeout), firing it
/// while the 300s sleep stays pending — deterministic, no wall-clock wait.
#[tokio::test(start_paused = true)]
async fn select_timeout_cancels_pending_second_poll() {
    let mut s = slow_stream(None);

    let first = s
        .next()
        .await
        .expect("stream ended early")
        .expect("chunk errored");
    assert_eq!(first, "first");

    let outcome = tokio::select! {
        () = tokio::time::sleep(Duration::from_secs(1)) => "timeout",
        _ = s.next() => "second-arrived",
    };
    assert_eq!(
        outcome, "timeout",
        "the slow second chunk must not arrive before the timeout"
    );
}

/// REAL cancellation: dropping the stream while the inner generator is parked on
/// an await runs that future's `Drop`, flipping a sentinel.
///
/// `elapsed < 1s`-style checks are tautological (Rust `Drop` is synchronous), so
/// this asserts an explicit teardown signal instead: the `DropGuard` only flips
/// the flag if the suspended generator (and its live locals) are actually
/// dropped when the stream is cancelled (acceptance #6).
#[tokio::test(start_paused = true)]
async fn drop_runs_inner_future_drop_guard() {
    let cancelled = Arc::new(AtomicBool::new(false));
    let mut s = slow_stream(Some(DropGuard(Arc::clone(&cancelled))));

    // First chunk drains; the generator parks at the first `yield` and the
    // guard does not exist yet.
    assert_eq!(s.next().await.unwrap().unwrap(), "first");
    assert!(
        !cancelled.load(Ordering::SeqCst),
        "guard must not exist before the second poll"
    );

    // A cancelled second poll resumes the generator into the guard + sleep
    // region, then parks it. The generator (and the live guard) are still owned
    // by `s`, so nothing has dropped yet.
    let outcome = tokio::select! {
        () = tokio::time::sleep(Duration::from_secs(1)) => "timeout",
        _ = s.next() => "second-arrived",
    };
    assert_eq!(outcome, "timeout");
    assert!(
        !cancelled.load(Ordering::SeqCst),
        "guard is live but still owned by the un-dropped stream"
    );

    // Cancel for real: dropping the stream drops the suspended generator, which
    // drops its live locals, which runs the guard's `Drop`.
    drop(s);
    assert!(
        cancelled.load(Ordering::SeqCst),
        "dropping the stream must tear down the in-flight inner future"
    );
}

/// Several independent in-memory streams drained concurrently all succeed; no
/// chunk is lost or turned into an error.
#[tokio::test]
async fn concurrent_streams_all_drain_ok() {
    async fn drain(chunks: &[&'static str]) -> Vec<honcho_ai::error::Result<String>> {
        let owned: Vec<honcho_ai::error::Result<String>> =
            chunks.iter().map(|c| Ok((*c).to_string())).collect();
        let mut s = DialecticStream::new(futures_util::stream::iter(owned));
        let mut out = Vec::new();
        while let Some(item) = s.next().await {
            out.push(item);
        }
        out
    }

    let (a, b) = tokio::join!(drain(&["good_before", " ", "after"]), drain(&["x", "y"]),);

    assert!(a.iter().all(Result::is_ok), "stream A yielded an error");
    assert!(b.iter().all(Result::is_ok), "stream B yielded an error");

    let texts: Vec<String> = a.into_iter().map(Result::unwrap).collect();
    assert!(
        texts.iter().any(|s| s == "good_before"),
        "the sentinel chunk must survive the drain"
    );
}

/// Dropping a streaming HTTP body mid-read closes the TCP connection for real.
///
/// A bare `verify()`/request-count check proves nothing about disconnect, so
/// this uses a hand-rolled socket server that streams one chunk and then blocks
/// on its read half: a clean EOF there is the client tearing the connection
/// down on body drop. Synchronisation is via a oneshot (no wall-clock sleep);
/// the only real-time wait is a generous safety timeout.
#[tokio::test]
async fn dropping_response_mid_body_disconnects_server() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let (disconnected_tx, disconnected_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();

        // Drain the request head (a bodyless GET arrives in one loopback segment).
        let mut req = [0u8; 1024];
        let _ = sock.read(&mut req).await.unwrap();

        // Chunked 200: headers + one 5-byte chunk, then deliberately NO
        // terminating 0-chunk, so the body stays incomplete and the client is
        // forced to read into a half-open stream.
        sock.write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n",
        )
        .await
        .unwrap();
        sock.write_all(b"5\r\nfirst\r\n").await.unwrap();
        sock.flush().await.unwrap();

        // Block on the read half. `Ok(0)` (EOF) or an error means the client
        // closed the socket — the real TCP disconnect we want to observe. A
        // read error ends the `while let` (it only matches `Ok`); `Ok(0)` breaks.
        let mut tail = [0u8; 64];
        while let Ok(n) = sock.read(&mut tail).await {
            if n == 0 {
                break;
            }
        }
        let _ = disconnected_tx.send(());
    });

    let url = format!("http://{addr}/stream");
    let resp = common::http_client().get(&url).send().await.unwrap();
    assert!(resp.status().is_success());

    let mut body = resp.bytes_stream();
    let first = body
        .next()
        .await
        .expect("server sent no body")
        .expect("body chunk errored");
    assert_eq!(&first[..], b"first");

    // Drop the streaming body before the response completes: hyper cannot reuse
    // a half-read connection, so it closes the socket. This is the cancellation
    // under test — and the reason the static `set_body_raw` approach can't show
    // it (that buffers the whole body up front).
    drop(body);

    tokio::time::timeout(Duration::from_secs(5), disconnected_rx)
        .await
        .expect("server did not observe a disconnect within 5s")
        .expect("server task dropped the sender before signalling");

    server.await.unwrap();
}
