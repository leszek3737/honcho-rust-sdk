//! Stream adapter that accumulates dialectic content for `final_response()`.

use std::pin::Pin;
use std::task::{Context, Poll};

use futures_util::Stream;
use futures_util::stream::FusedStream;

use crate::error::Result;

/// The fully-accumulated content of a completed dialectic stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalResponse {
    content: String,
}

impl FinalResponse {
    /// Create a new `FinalResponse` from the accumulated content.
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
        }
    }

    /// Access the accumulated response text.
    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }
}

impl std::fmt::Display for FinalResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.content)
    }
}

/// Stream adapter that accumulates content from a dialectic SSE stream.
///
/// Wraps any `Stream<Item = Result<String>>` and builds a [`FinalResponse`]
/// from all yielded chunks. The stream is pass-through — callers still
/// receive each chunk individually while the adapter silently accumulates.
///
/// Note: accumulation is unbounded — the `FinalResponse` buffer grows with the
/// total length of the stream. This mirrors the upstream SDKs (which buffer the
/// full dialectic response) and is fine for chat-sized responses, but callers
/// driving arbitrarily long streams should be aware of the memory cost.
pub struct DialecticStream<S> {
    inner: S,
    final_response: FinalResponse,
    complete: bool,
    errored: bool,
}

#[allow(clippy::missing_fields_in_debug)]
impl<S> std::fmt::Debug for DialecticStream<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DialecticStream")
            .field("is_complete", &self.complete)
            .field("is_errored", &self.errored)
            .field("content_len", &self.final_response.content.len())
            .finish()
    }
}

impl<S> DialecticStream<S>
where
    S: Stream<Item = Result<String>> + Unpin,
{
    /// Initial capacity reserved for the accumulation buffer.
    const INITIAL_CAPACITY: usize = 1024;

    /// Wrap a stream, accumulating all successful content chunks.
    pub fn new(stream: S) -> Self {
        Self {
            inner: stream,
            final_response: FinalResponse {
                content: String::with_capacity(Self::INITIAL_CAPACITY),
            },
            complete: false,
            errored: false,
        }
    }

    /// Returns the accumulated response so far (partial if stream is still in progress).
    #[must_use]
    pub fn final_response(&self) -> &FinalResponse {
        &self.final_response
    }

    /// `true` once the stream has terminated — either the inner stream returned
    /// `None` (clean end-of-stream) or it yielded an error (see [`is_errored`]).
    ///
    /// [`is_errored`]: Self::is_errored
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.complete
    }

    /// `true` if the stream terminated because the inner stream yielded an error.
    ///
    /// When this is `true`, [`final_response`] holds only the content
    /// accumulated *before* the error and must not be treated as complete.
    ///
    /// [`final_response`]: Self::final_response
    #[must_use]
    pub fn is_errored(&self) -> bool {
        self.errored
    }
}

impl<S> Stream for DialecticStream<S>
where
    S: Stream<Item = Result<String>> + Unpin,
{
    type Item = Result<String>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // `Stream` contract: once a stream has yielded `Ready(None)` it must not
        // be polled again — doing so is allowed to panic. We also treat an error
        // as terminal. Guard *before* touching `inner` so a fused/exhausted inner
        // stream is never re-polled.
        if self.complete {
            return Poll::Ready(None);
        }

        match Pin::new(&mut self.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(content))) => {
                self.final_response.content.push_str(&content);
                Poll::Ready(Some(Ok(content)))
            }
            Poll::Ready(Some(Err(e))) => {
                // An error terminates the stream: mark it complete so we never
                // poll `inner` again, and record the error so `is_errored()` can
                // distinguish a partial (failed) buffer from a clean end-of-stream.
                self.complete = true;
                self.errored = true;
                Poll::Ready(Some(Err(e)))
            }
            Poll::Ready(None) => {
                self.complete = true;
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        if self.complete {
            return (0, Some(0));
        }
        self.inner.size_hint()
    }
}

impl<S> FusedStream for DialecticStream<S>
where
    S: Stream<Item = Result<String>> + Unpin,
{
    fn is_terminated(&self) -> bool {
        self.complete
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unnecessary_wraps,
    clippy::needless_pass_by_value,
    clippy::unused_async
)]
mod tests {
    use futures_util::StreamExt;

    use super::*;
    use crate::error::HonchoError;

    fn ok_chunk(s: &str) -> Result<String> {
        Ok(s.to_string())
    }

    #[tokio::test]
    async fn dialectic_stream_accumulates_during_iteration() {
        let chunks = vec![ok_chunk("hello"), ok_chunk(" "), ok_chunk("world")];
        let inner = futures_util::stream::iter(chunks);
        let mut stream = DialecticStream::new(inner);

        let mut collected = Vec::new();
        while let Some(item) = stream.next().await {
            collected.push(item.unwrap());
        }

        assert_eq!(collected, vec!["hello", " ", "world"]);
        assert_eq!(stream.final_response().content(), "hello world");
    }

    #[tokio::test]
    async fn dialectic_stream_is_complete_after_done() {
        let chunks = vec![ok_chunk("a"), ok_chunk("b")];
        let inner = futures_util::stream::iter(chunks);
        let mut stream = DialecticStream::new(inner);

        assert!(!stream.is_complete());

        while stream.next().await.is_some() {}

        assert!(stream.is_complete());
        assert_eq!(stream.final_response().content(), "ab");
    }

    #[tokio::test]
    async fn dialectic_stream_final_response_before_completion_returns_partial() {
        let chunks = vec![ok_chunk("first"), ok_chunk(" second")];
        let inner = futures_util::stream::iter(chunks);
        let mut stream = DialecticStream::new(inner);

        let first = stream.next().await.unwrap().unwrap();
        assert_eq!(first, "first");
        assert_eq!(stream.final_response().content(), "first");
        assert!(!stream.is_complete());
    }

    #[tokio::test]
    async fn dialectic_stream_propagates_errors() {
        let chunks: Vec<Result<String>> = vec![
            ok_chunk("ok"),
            Err(HonchoError::Connection {
                message: "boom".to_string(),
            }),
        ];
        let inner = futures_util::stream::iter(chunks);
        let mut stream = DialecticStream::new(inner);

        let first = stream.next().await.unwrap().unwrap();
        assert_eq!(first, "ok");

        let err = stream.next().await.unwrap().unwrap_err();
        assert!(matches!(err, HonchoError::Connection { .. }));
        assert_eq!(stream.final_response().content(), "ok");
    }

    #[tokio::test]
    async fn dialectic_stream_empty_input() {
        let chunks: Vec<Result<String>> = vec![];
        let inner = futures_util::stream::iter(chunks);
        let mut stream = DialecticStream::new(inner);

        assert!(stream.next().await.is_none());
        assert!(stream.is_complete());
        assert_eq!(stream.final_response().content(), "");
    }

    #[tokio::test]
    async fn dialectic_stream_final_response_display() {
        let chunks = vec![ok_chunk("hello")];
        let inner = futures_util::stream::iter(chunks);
        let mut stream = DialecticStream::new(inner);
        while stream.next().await.is_some() {}

        assert_eq!(stream.final_response().to_string(), "hello");
    }

    #[tokio::test]
    async fn dialectic_stream_final_response_eq() {
        let chunks = vec![ok_chunk("abc")];
        let inner = futures_util::stream::iter(chunks);
        let mut stream = DialecticStream::new(inner);
        while stream.next().await.is_some() {}

        let expected = FinalResponse {
            content: "abc".to_string(),
        };
        assert_eq!(stream.final_response(), &expected);
    }

    #[tokio::test]
    async fn dialectic_stream_handles_pending_then_ready() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<String>>(16);

        let inner = async_stream::stream! {
            while let Some(item) = rx.recv().await {
                yield item;
            }
        };
        let mut stream = DialecticStream::new(Box::pin(inner));

        // Stream has no data yet — send after a short delay to simulate backpressure
        let handle = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            tx.send(Ok("delayed-chunk".to_string())).await.ok();
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            tx.send(Ok("-more".to_string())).await.ok();
        });

        let mut collected = Vec::new();
        while let Some(item) = stream.next().await {
            collected.push(item.unwrap());
        }
        handle.await.ok();

        assert_eq!(collected, vec!["delayed-chunk", "-more"]);
        assert_eq!(stream.final_response().content(), "delayed-chunk-more");
        assert!(stream.is_complete());
    }

    /// Inner stream that yields `Ready(None)` exactly once and then panics if
    /// polled again. Used to prove `DialecticStream` honours the `Stream`
    /// contract and never re-polls a terminated inner stream.
    struct PanicAfterEnd {
        yielded_none: bool,
    }

    impl Stream for PanicAfterEnd {
        type Item = Result<String>;

        fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            assert!(
                !self.yielded_none,
                "inner stream polled again after returning Ready(None)"
            );
            self.yielded_none = true;
            Poll::Ready(None)
        }
    }

    #[tokio::test]
    async fn poll_next_after_end_does_not_touch_inner() {
        let inner = PanicAfterEnd {
            yielded_none: false,
        };
        let mut stream = DialecticStream::new(inner);

        // First poll: inner returns None, adapter marks itself complete.
        assert!(stream.next().await.is_none());
        assert!(stream.is_complete());

        // Subsequent polls must return Ready(None) WITHOUT polling inner —
        // if the guard were missing, `PanicAfterEnd` would panic here.
        assert!(stream.next().await.is_none());
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn error_sets_complete_and_errored_state() {
        let chunks: Vec<Result<String>> = vec![
            ok_chunk("partial"),
            Err(HonchoError::Connection {
                message: "boom".to_string(),
            }),
        ];
        let inner = futures_util::stream::iter(chunks);
        let mut stream = DialecticStream::new(inner);

        assert_eq!(stream.next().await.unwrap().unwrap(), "partial");
        assert!(!stream.is_complete());
        assert!(!stream.is_errored());

        let err = stream.next().await.unwrap().unwrap_err();
        assert!(matches!(err, HonchoError::Connection { .. }));

        // An error is terminal: complete + errored are both set, and the
        // accumulated buffer holds only the pre-error content.
        assert!(stream.is_complete());
        assert!(stream.is_errored());
        assert_eq!(stream.final_response().content(), "partial");

        // No further items are produced after the terminal error.
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn fused_stream_is_terminated_tracks_completion() {
        let inner = futures_util::stream::iter(vec![ok_chunk("x")]);
        let mut stream = DialecticStream::new(inner);

        assert!(!stream.is_terminated());
        while stream.next().await.is_some() {}
        assert!(stream.is_terminated());
    }
}
