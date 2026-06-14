//! Incremental SSE stream parser for dialectic streaming.
//!
//! Byte-stream level parser that handles split UTF-8 codepoints and split lines
//! across arbitrary network chunk boundaries. Parity with Python `sse.py`.

use futures_util::StreamExt;
use serde_json::Value;

/// Maximum buffer size before an unterminated line is treated as an error.
/// Prevents OOM from a malicious server that never sends newlines.
const MAX_SSE_LINE_BYTES: usize = 1 << 20; // 1 MiB

/// Incremental SSE parser that extracts `delta.content` strings from a
/// `data: <json>` line stream.
///
/// Each `data:` line is parsed as an independent JSON object (not concatenated
/// multi-line). Lines are split on `\n`, `\r`, or `\r\n`.
pub(crate) struct SseParser {
    buffer: String,
    pending_bytes: Vec<u8>,
    done: bool,
}

impl SseParser {
    /// Create a new parser.
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            pending_bytes: Vec::new(),
            done: false,
        }
    }

    /// Feed a byte chunk from the SSE stream, returning extracted content strings.
    ///
    /// # Errors
    /// Returns `HonchoError::Connection` if a single unterminated line exceeds
    /// `MAX_SSE_LINE_BYTES` (`DoS` guard).
    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<String>, crate::error::HonchoError> {
        if self.done || chunk.is_empty() {
            return Ok(Vec::new());
        }
        self.decode_pending(chunk);
        let lines = self.drain_lines(false);
        // DoS guard: after draining all completed lines, whatever remains is a
        // single unterminated line (plus any undecoded trailing bytes). Only
        // that retained leftover is bounded — a large chunk made of many
        // complete lines drains away and is fine.
        if self.buffer.len().saturating_add(self.pending_bytes.len()) > MAX_SSE_LINE_BYTES {
            return Err(crate::error::HonchoError::Connection {
                message: format!("SSE line exceeded maximum length of {MAX_SSE_LINE_BYTES} bytes"),
            });
        }
        Ok(lines)
    }

    /// Flush remaining bytes and return any final content strings.
    ///
    /// # Errors
    /// Returns `HonchoError::Connection` if the buffered, unterminated line
    /// exceeds `MAX_SSE_LINE_BYTES` (`DoS` guard).
    pub fn finalize(&mut self) -> Result<Vec<String>, crate::error::HonchoError> {
        if self.done {
            return Ok(Vec::new());
        }
        if !self.pending_bytes.is_empty() {
            let lossy = String::from_utf8_lossy(&self.pending_bytes);
            self.buffer.push_str(&lossy);
            self.pending_bytes.clear();
        }
        if self.buffer.len() > MAX_SSE_LINE_BYTES {
            return Err(crate::error::HonchoError::Connection {
                message: format!("SSE line exceeded maximum length of {MAX_SSE_LINE_BYTES} bytes"),
            });
        }
        Ok(self.drain_lines(true))
    }

    /// Whether the stream has emitted a `done: true` message.
    #[must_use]
    pub fn done(&self) -> bool {
        self.done
    }

    fn decode_pending(&mut self, chunk: &[u8]) {
        self.pending_bytes.extend_from_slice(chunk);
        let mut start = 0;
        while start < self.pending_bytes.len() {
            match std::str::from_utf8(&self.pending_bytes[start..]) {
                Ok(s) => {
                    self.buffer.push_str(s);
                    self.pending_bytes.clear();
                    return;
                }
                Err(e) => {
                    let valid_up_to = e.valid_up_to();
                    if valid_up_to > 0 {
                        let valid_slice =
                            std::str::from_utf8(&self.pending_bytes[start..start + valid_up_to]);
                        debug_assert!(valid_slice.is_ok(), "valid_up_to returned invalid UTF-8");
                        self.buffer.push_str(valid_slice.unwrap_or(""));
                        start += valid_up_to;
                    }
                    match e.error_len() {
                        Some(bad_len) => {
                            self.buffer.push('\u{FFFD}');
                            start += bad_len;
                        }
                        None => break,
                    }
                }
            }
        }
        if start > 0 {
            self.pending_bytes.drain(..start);
        }
    }

    fn drain_lines(&mut self, flush_partial: bool) -> Vec<String> {
        let mut results = Vec::new();
        while !self.done {
            let Some(line) = self.pop_line(flush_partial) else {
                break;
            };
            if let Some(content) = self.handle_line(&line) {
                results.push(content);
            }
        }
        results
    }

    fn pop_line(&mut self, flush_partial: bool) -> Option<String> {
        if self.buffer.is_empty() {
            return None;
        }

        let idx_n = self.buffer.find('\n');
        let idx_r = self.buffer.find('\r');

        match (idx_n, idx_r) {
            (None, None) => {
                if flush_partial {
                    Some(std::mem::take(&mut self.buffer))
                } else {
                    None
                }
            }
            (Some(n), None) => {
                let line = self.buffer[..n].to_string();
                self.buffer.drain(..=n);
                Some(line)
            }
            (None, Some(r)) => {
                if r == self.buffer.len() - 1 && !flush_partial {
                    return None;
                }
                let end = if r + 1 < self.buffer.len() && self.buffer.as_bytes()[r + 1] == b'\n' {
                    r + 2
                } else {
                    r + 1
                };
                let line = self.buffer[..r].to_string();
                self.buffer.drain(..end);
                Some(line)
            }
            (Some(n), Some(r)) => {
                if r < n {
                    let end = if r + 1 < self.buffer.len() && self.buffer.as_bytes()[r + 1] == b'\n'
                    {
                        r + 2
                    } else {
                        r + 1
                    };
                    let line = self.buffer[..r].to_string();
                    self.buffer.drain(..end);
                    Some(line)
                } else {
                    let line = self.buffer[..n].to_string();
                    self.buffer.drain(..=n);
                    Some(line)
                }
            }
        }
    }

    fn handle_line(&mut self, line: &str) -> Option<String> {
        let rest = line.strip_prefix("data:")?;
        let json_str = rest.strip_prefix(' ').unwrap_or(rest);
        if json_str.is_empty() {
            return None;
        }

        let parsed: Value = match serde_json::from_str(json_str) {
            Ok(v) => v,
            #[cfg(feature = "tracing")]
            Err(e) => {
                tracing::warn!(
                    "Failed to decode streaming chunk: {} (data: {})",
                    e,
                    &json_str[..json_str
                        .char_indices()
                        .nth(100)
                        .map_or(json_str.len(), |(i, _)| i)]
                );
                return None;
            }
            #[cfg(not(feature = "tracing"))]
            Err(_) => {
                return None;
            }
        };

        let obj = parsed.as_object()?;

        if let Some(done_val) = obj.get("done")
            && done_val.as_bool().unwrap_or(false)
        {
            self.done = true;
            return None;
        }

        let delta = obj.get("delta")?.as_object()?;
        let content = delta.get("content")?;
        match content.as_str() {
            Some(s) if !s.is_empty() => Some(s.to_string()),
            _ => None,
        }
    }
}

impl Default for SseParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse a byte stream of SSE data into a stream of content strings.
///
/// # Errors
/// Returns `HonchoError::Connection` if the underlying byte stream errors,
/// or if an SSE line exceeds `MAX_SSE_LINE_BYTES` (`DoS` guard).
pub fn parse_sse_stream(
    stream: impl futures_util::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
) -> impl futures_util::Stream<Item = Result<String, crate::error::HonchoError>> + Send + 'static {
    let mut parser = SseParser::new();
    let mut stream: std::pin::Pin<
        Box<dyn futures_util::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send>,
    > = Box::pin(stream);

    async_stream::try_stream! {
        while let Some(chunk_result) = stream.next().await {
            match chunk_result {
                Ok(chunk) => {
                    for content in parser.feed(&chunk)? {
                        yield content;
                    }
                    if parser.done() {
                        break;
                    }
                }
                Err(e) => {
                    // Flush buffered content before propagating the error.
                    // If finalize() itself fails its DoS guard, surface that
                    // error instead of swallowing it (mirrors the feed()? path).
                    match parser.finalize() {
                        Ok(items) => {
                            for content in items {
                                yield content;
                            }
                        }
                        Err(fin_err) => {
                            yield Err(fin_err)?;
                        }
                    }
                    yield Err(crate::error::HonchoError::Connection {
                        message: e.to_string(),
                    })?;
                }
            }
        }

        for content in parser.finalize()? {
            yield content;
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn data_line(json: &str) -> Vec<u8> {
        format!("data: {json}\n\n").into_bytes()
    }

    #[test]
    fn f8_2_2_basic_data_line() {
        let mut p = SseParser::new();
        let r = p
            .feed(&data_line(r#"{"delta":{"content":"hello"}}"#))
            .unwrap();
        assert_eq!(r, vec!["hello"]);
        assert!(!p.done());
    }

    #[test]
    fn f8_2_3_done_flag() {
        let mut p = SseParser::new();
        let r = p.feed(&data_line(r#"{"done":true}"#)).unwrap();
        assert!(r.is_empty());
        assert!(p.done());
    }

    #[test]
    fn f8_2_4_consecutive_chunks() {
        let mut p = SseParser::new();
        let mut all = Vec::new();
        all.extend(
            p.feed(&data_line(r#"{"delta":{"content":"hello"}}"#))
                .unwrap(),
        );
        all.extend(
            p.feed(&data_line(r#"{"delta":{"content":" world"}}"#))
                .unwrap(),
        );
        assert_eq!(all, vec!["hello", " world"]);
    }

    #[test]
    fn f8_2_5_split_utf8_codepoint() {
        let mut full: Vec<u8> = b"data: {\"delta\":{\"content\":\"abc".to_vec();
        full.extend_from_slice("\u{00e9}".as_bytes());
        full.extend_from_slice(b"\"}}\n\n");

        let split_pos = full.iter().position(|&b| b == 0xC3).map_or(0, |p| p + 1);

        let mut p = SseParser::new();
        let mut results = Vec::new();
        results.extend(p.feed(&full[..split_pos]).unwrap());
        results.extend(p.feed(&full[split_pos..]).unwrap());
        assert_eq!(results, vec!["abc\u{00e9}"]);
    }

    #[test]
    fn f8_2_6_split_line_across_chunks() {
        let full = b"data: {\"delta\":{\"content\":\"hello\"}}\n\n";
        let mid = full.len() / 2;

        let mut p = SseParser::new();
        let mut results = Vec::new();
        results.extend(p.feed(&full[..mid]).unwrap());
        assert!(results.is_empty());
        results.extend(p.feed(&full[mid..]).unwrap());
        assert_eq!(results, vec!["hello"]);
    }

    #[test]
    fn f8_2_7_ignore_non_data_lines() {
        let mut p = SseParser::new();
        let mut all = Vec::new();
        all.extend(p.feed(b": heartbeat\n").unwrap());
        all.extend(p.feed(b"event: foo\n").unwrap());
        all.extend(
            p.feed(&data_line(r#"{"delta":{"content":"yes"}}"#))
                .unwrap(),
        );
        assert_eq!(all, vec!["yes"]);
    }

    #[test]
    fn f8_2_8_empty_data_line() {
        let mut p = SseParser::new();
        let r = p
            .feed(b"data:\n\ndata: {\"delta\":{\"content\":\"x\"}}\n\n")
            .unwrap();
        assert_eq!(r, vec!["x"]);
    }

    #[test]
    fn f8_2_9_data_without_space() {
        let mut p = SseParser::new();
        let r = p
            .feed(b"data:{\"delta\":{\"content\":\"nospace\"}}\n\n")
            .unwrap();
        assert_eq!(r, vec!["nospace"]);
    }

    #[test]
    fn f8_2_10_malformed_json() {
        let mut p = SseParser::new();
        let r = p.feed(b"data: {garbage\n\n").unwrap();
        assert!(r.is_empty());
        assert!(!p.done());
    }

    #[test]
    fn f8_2_11_crlf() {
        let mut p = SseParser::new();
        let r = p
            .feed(b"data: {\"delta\":{\"content\":\"crlf\"}}\r\n\r\n")
            .unwrap();
        assert_eq!(r, vec!["crlf"]);
    }

    #[test]
    fn f8_2_12_lf_only() {
        let mut p = SseParser::new();
        let r = p
            .feed(b"data: {\"delta\":{\"content\":\"lf\"}}\n\n")
            .unwrap();
        assert_eq!(r, vec!["lf"]);
    }

    #[test]
    fn f8_2_13_cr_only() {
        let mut p = SseParser::new();
        let r = p
            .feed(b"data: {\"delta\":{\"content\":\"cr\"}}\r\r")
            .unwrap();
        assert_eq!(r, vec!["cr"]);
    }

    #[test]
    fn f8_2_14_delta_without_content() {
        let mut p = SseParser::new();
        let r = p.feed(&data_line(r#"{"delta":{}}"#)).unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn f8_2_15_non_string_content() {
        let mut p = SseParser::new();
        let r = p.feed(&data_line(r#"{"delta":{"content":42}}"#)).unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn f8_2_16_non_dict_json() {
        let mut p = SseParser::new();
        let r = p.feed(b"data: [\"not\",\"dict\"]\n\n").unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn f8_2_17_finalize_flushes_remaining() {
        let mut p = SseParser::new();
        p.feed(b"data: {\"delta\":{\"content\":\"partial\"}}")
            .unwrap();
        let r = p.finalize().unwrap();
        assert_eq!(r, vec!["partial"]);
    }

    #[test]
    fn done_stops_further_feeds() {
        let mut p = SseParser::new();
        let mut all = Vec::new();
        all.extend(
            p.feed(&data_line(r#"{"delta":{"content":"before"}}"#))
                .unwrap(),
        );
        all.extend(p.feed(&data_line(r#"{"done":true}"#)).unwrap());
        all.extend(
            p.feed(&data_line(r#"{"delta":{"content":"after"}}"#))
                .unwrap(),
        );
        assert_eq!(all, vec!["before"]);
        assert!(p.done());
    }

    #[test]
    fn finalize_after_done_yields_nothing() {
        let mut p = SseParser::new();
        p.feed(&data_line(r#"{"done":true}"#)).unwrap();
        let r = p.finalize().unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn empty_chunk_returns_empty() {
        let mut p = SseParser::new();
        let r = p.feed(b"").unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn invalid_utf8_byte_does_not_block_subsequent_valid_bytes() {
        let mut p = SseParser::new();

        let mut chunk1 = b"data: {\"delta\":{\"content\":\"".to_vec();
        chunk1.push(0xFF);
        chunk1.extend_from_slice(b"\"}}\n\n");
        let r1 = p.feed(&chunk1).unwrap();
        assert_eq!(r1, vec!["\u{FFFD}"]);

        let r2 = p.feed(&data_line(r#"{"delta":{"content":"ok"}}"#)).unwrap();
        assert_eq!(r2, vec!["ok"]);
    }

    #[test]
    fn default_impl() {
        let p = SseParser::default();
        assert!(!p.done());
    }

    #[test]
    fn multiple_data_lines_in_one_chunk() {
        let mut p = SseParser::new();
        let input = b"data: {\"delta\":{\"content\":\"a\"}}\n\
                      data: {\"delta\":{\"content\":\"b\"}}\n\n";
        let r = p.feed(input).unwrap();
        assert_eq!(r, vec!["a", "b"]);
    }

    #[test]
    fn feed_rejects_oversized_line() {
        let mut p = SseParser::new();
        let huge = vec![b'a'; MAX_SSE_LINE_BYTES + 1];
        let result = p.feed(&huge);
        assert!(result.is_err());
    }

    #[test]
    fn feed_rejects_unterminated_line_across_chunks() {
        // No single chunk exceeds the limit, but an unterminated line that keeps
        // accumulating across feeds must still be rejected.
        let mut p = SseParser::new();
        let half = vec![b'a'; (MAX_SSE_LINE_BYTES / 2) + 1];
        assert!(p.feed(&half).is_ok());
        assert!(p.feed(&half).is_err());
    }

    #[test]
    fn feed_accepts_large_chunk_of_complete_lines() {
        // A chunk far larger than MAX_SSE_LINE_BYTES is fine as long as every
        // line is terminated — completed lines drain away and never accumulate.
        // Use few, large lines (rather than many tiny ones) so the test stays
        // fast despite pop_line's O(n) drain per line.
        let mut p = SseParser::new();
        let content = "x".repeat(2048);
        let line = format!("data: {{\"delta\":{{\"content\":\"{content}\"}}}}\n");
        let line_count = (MAX_SSE_LINE_BYTES / line.len()) + 2;
        let chunk = line.repeat(line_count);
        assert!(chunk.len() > MAX_SSE_LINE_BYTES);

        let r = p
            .feed(chunk.as_bytes())
            .expect("large chunk of complete lines must be accepted");
        assert_eq!(r.len(), line_count);
        assert!(r.iter().all(|s| s == &content));
    }

    #[test]
    fn finalize_rejects_oversized_line() {
        let mut p = SseParser::new();
        // Simulate buffer that grew beyond limit through pending_bytes accumulation
        // (feed()'s pre-allocation check normally prevents this, but finalize()
        // must still guard against the edge case)
        p.buffer = "a".repeat(MAX_SSE_LINE_BYTES + 1);
        let result = p.finalize();
        assert!(result.is_err());
    }

    // ── parse_sse_stream tests (F8.3.1–F8.3.4) ──────────────────────────

    fn data_line_bytes(json: &str) -> bytes::Bytes {
        format!("data: {json}\n\n").into_bytes().into()
    }

    async fn collect_stream(
        s: impl futures_util::Stream<Item = Result<String, crate::error::HonchoError>>,
    ) -> Vec<Result<String, crate::error::HonchoError>> {
        futures_util::StreamExt::collect(s).await
    }

    #[tokio::test]
    async fn parse_sse_stream_yields_all_content_until_done() {
        let chunks: Vec<Result<bytes::Bytes, reqwest::Error>> = vec![
            Ok(data_line_bytes(r#"{"delta":{"content":"hello"}}"#)),
            Ok(data_line_bytes(r#"{"delta":{"content":" world"}}"#)),
            Ok(data_line_bytes(r#"{"delta":{"content":"!"}}"#)),
        ];
        let stream = futures_util::stream::iter(chunks);
        let results = collect_stream(parse_sse_stream(stream)).await;

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].as_ref().unwrap(), "hello");
        assert_eq!(results[1].as_ref().unwrap(), " world");
        assert_eq!(results[2].as_ref().unwrap(), "!");
    }

    #[tokio::test]
    async fn parse_sse_stream_terminates_on_done_flag_even_with_trailing_bytes() {
        let chunks: Vec<Result<bytes::Bytes, reqwest::Error>> = vec![
            Ok(data_line_bytes(r#"{"delta":{"content":"first"}}"#)),
            Ok(data_line_bytes(r#"{"done":true}"#)),
            Ok(data_line_bytes(r#"{"delta":{"content":"ignored"}}"#)),
        ];
        let stream = futures_util::stream::iter(chunks);
        let results = collect_stream(parse_sse_stream(stream)).await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].as_ref().unwrap(), "first");
    }

    #[tokio::test]
    async fn parse_sse_stream_finalizes_on_eof() {
        let partial: bytes::Bytes = b"data: {\"delta\":{\"content\":\"partial\"}}"
            .to_vec()
            .into();
        let chunks: Vec<Result<bytes::Bytes, reqwest::Error>> = vec![Ok(partial)];
        let stream = futures_util::stream::iter(chunks);
        let results = collect_stream(parse_sse_stream(stream)).await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].as_ref().unwrap(), "partial");
    }

    #[tokio::test]
    async fn parse_sse_stream_propagates_io_error_from_byte_stream() {
        let error = reqwest::Client::builder()
            .timeout(std::time::Duration::from_micros(1))
            .build()
            .unwrap()
            .get("http://127.0.0.1:1")
            .send()
            .await
            .unwrap_err();
        let chunks: Vec<Result<bytes::Bytes, reqwest::Error>> = vec![
            Ok(data_line_bytes(r#"{"delta":{"content":"before_err"}}"#)),
            Err(error),
        ];
        let stream = futures_util::stream::iter(chunks);
        let results = collect_stream(parse_sse_stream(stream)).await;

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].as_ref().unwrap(), "before_err");
        assert!(
            matches!(
                results[1],
                Err(crate::error::HonchoError::Connection { .. })
            ),
            "expected Connection error, got {:?}",
            results[1]
        );
    }

    #[test]
    fn finalize_returns_buffered_content_from_multiple_partial_feeds() {
        let mut p = SseParser::new();
        // Feed a partial data line split across two feed calls — no newline
        p.feed(b"data: {\"delta\":{\"content\":\"part").unwrap();
        p.feed(b"_two\"}}").unwrap();
        let r = p.finalize().unwrap();
        assert_eq!(r, vec!["part_two"]);
    }

    #[tokio::test]
    async fn parse_sse_stream_flushes_buffered_content_before_error() {
        // Complete data line, then a partial data line (no newline) that stays
        // in the buffer, then an error. The buffered content should be flushed
        // via finalize() before the error propagates.
        let error = reqwest::Client::builder()
            .timeout(std::time::Duration::from_micros(1))
            .build()
            .unwrap()
            .get("http://127.0.0.1:1")
            .send()
            .await
            .unwrap_err();

        let chunks: Vec<Result<bytes::Bytes, reqwest::Error>> = vec![
            Ok(data_line_bytes(r#"{"delta":{"content":"hello"}}"#)),
            Ok(b"data: {\"delta\":{\"content\":\"buffered\"}}"
                .to_vec()
                .into()),
            Err(error),
        ];
        let stream = futures_util::stream::iter(chunks);
        let results = collect_stream(parse_sse_stream(stream)).await;

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].as_ref().unwrap(), "hello");
        assert_eq!(results[1].as_ref().unwrap(), "buffered");
        assert!(
            matches!(
                results[2],
                Err(crate::error::HonchoError::Connection { .. })
            ),
            "expected Connection error, got {:?}",
            results[2]
        );
    }

    // ── R-34 SSE cancellation-safety tests ──────────────────────────────
    //
    // Recovered from the former `tests/sse_cancel.rs`, ported inline now that
    // `http` is `pub(crate)` and unreachable from an external test crate:
    //   1. `tokio::select!` + drop cancels a slow SSE stream without hanging
    //   2. Dropping the stream mid-read is visible as a TCP disconnect (wiremock)
    //   3. Malformed JSON mid-stream never panics; valid content around it is
    //      still yielded
    //   4. `DialecticStream` wrapper cancels cleanly via `tokio::select!`
    mod cancel_safety {
        #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

        use std::pin::Pin;
        use std::time::{Duration, Instant};

        use bytes::Bytes;
        use futures_util::StreamExt;

        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        use crate::error::HonchoError;
        use crate::http::sse::parse_sse_stream;

        fn sse_chunk(data: &str) -> String {
            format!("data: {data}\n\n")
        }

        // Test 1: tokio::select! cancels slow stream, drop completes fast
        #[tokio::test]
        async fn tokio_select_cancel_drops_stream_cleanly() {
            let slow_bytes: Pin<
                Box<dyn futures_util::Stream<Item = Result<Bytes, reqwest::Error>> + Send>,
            > = Box::pin(async_stream::stream! {
                yield Ok(Bytes::from(
                    "data: {\"delta\":{\"content\":\"first\"}}\n\n",
                ));
                tokio::time::sleep(Duration::from_secs(300)).await;
                yield Ok(Bytes::from(
                    "data: {\"delta\":{\"content\":\"never\"}}\n\n",
                ));
            });

            let mut s = Box::pin(parse_sse_stream(slow_bytes));

            let result = tokio::select! {
                chunk = s.next() => chunk,
                () = tokio::time::sleep(Duration::from_secs(5)) => {
                    panic!("timed out waiting for first SSE chunk");
                }
            };

            let content = result.expect("stream ended unexpectedly").unwrap();
            assert_eq!(content, "first");

            // Drop while inner stream still sleeping — must complete without blocking
            let before = Instant::now();
            drop(s);
            let elapsed = before.elapsed();

            assert!(
                elapsed < Duration::from_secs(1),
                "drop took {elapsed:?} — possible resource leak or blocking on Drop",
            );
        }

        // Test 2: cancel-safety — drop stream, assert TCP disconnect
        #[tokio::test]
        async fn drop_stream_causes_tcp_disconnect() {
            let server = MockServer::start().await;

            let first_chunk = sse_chunk(r#"{"delta":{"content":"chunk1"}}"#);

            let body = format!("{first_chunk}{{slow}}");

            Mock::given(method("POST"))
                .and(path("/v1/test/sse"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_raw(body.as_bytes(), "text/event-stream"),
                )
                .expect(1)
                .mount(&server)
                .await;

            let resp = reqwest::Client::new()
                .post(format!("{}{}", server.uri(), "/v1/test/sse"))
                .header("accept", "text/event-stream")
                .send()
                .await
                .unwrap();

            let byte_stream = resp.bytes_stream();
            let mut s: Pin<
                Box<dyn futures_util::Stream<Item = Result<String, HonchoError>> + Send>,
            > = Box::pin(parse_sse_stream(byte_stream));

            let first = s.next().await.expect("should yield first chunk").unwrap();
            assert_eq!(first, "chunk1");

            drop(s);

            tokio::time::sleep(Duration::from_millis(200)).await;

            server.verify().await;
        }

        // Test 3: malformed JSON mid-stream does not panic
        #[tokio::test]
        async fn malformed_json_mid_stream_no_panic() {
            let server = MockServer::start().await;

            let chunk1 = sse_chunk(r#"{"delta":{"content":"good_before"}}"#);
            let bad = "data: {not valid json!!!\n\n".to_string();
            let chunk3 = sse_chunk(r#"{"delta":{"content":"good_after"}}"#);
            let body = format!("{chunk1}{bad}{chunk3}");

            Mock::given(method("POST"))
                .and(path("/v1/test/sse"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_raw(body.as_bytes(), "text/event-stream"),
                )
                .mount(&server)
                .await;

            let resp = reqwest::Client::new()
                .post(format!("{}{}", server.uri(), "/v1/test/sse"))
                .header("accept", "text/event-stream")
                .send()
                .await
                .unwrap();

            let s = parse_sse_stream(resp.bytes_stream());
            let results: Vec<Result<String, HonchoError>> = s.collect().await;

            let ok_results: Vec<String> = results
                .into_iter()
                .filter_map(std::result::Result::ok)
                .collect();

            assert!(
                ok_results.contains(&"good_before".to_string()),
                "should yield content before malformed JSON"
            );
            assert!(
                ok_results.contains(&"good_after".to_string()),
                "should yield content after malformed JSON"
            );
            assert_eq!(ok_results.len(), 2, "only valid chunks should appear");
        }

        // Test 4: DialecticStream wrapper cancels cleanly via tokio::select!
        #[tokio::test]
        async fn dialectic_stream_cancel_via_select() {
            use crate::DialecticStream;

            let slow_bytes: Pin<
                Box<dyn futures_util::Stream<Item = Result<Bytes, reqwest::Error>> + Send>,
            > = Box::pin(async_stream::stream! {
                yield Ok(Bytes::from(
                    "data: {\"delta\":{\"content\":\"hello\"}}\n\n",
                ));
                tokio::time::sleep(Duration::from_secs(300)).await;
                yield Ok(Bytes::from(
                    "data: {\"delta\":{\"content\":\"world\"}}\n\n",
                ));
            });

            let inner = parse_sse_stream(slow_bytes);
            let mut ds = DialecticStream::new(Box::pin(inner));

            let item = tokio::select! {
                chunk = ds.next() => chunk,
                () = tokio::time::sleep(Duration::from_secs(5)) => {
                    panic!("timed out waiting for DialecticStream chunk");
                }
            };

            let content = item.expect("stream ended").unwrap();
            assert_eq!(content, "hello");
            assert_eq!(ds.final_response().content(), "hello");
            assert!(!ds.is_complete());

            let before = Instant::now();
            drop(ds);
            let elapsed = before.elapsed();

            assert!(
                elapsed < Duration::from_secs(1),
                "DialecticStream drop took {elapsed:?} — cancellation not clean",
            );
        }
    }
}
