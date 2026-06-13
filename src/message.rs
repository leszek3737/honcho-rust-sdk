//! Message wrapper — construction, getters, custom Debug/Display.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::types::message::MessageResponse;

/// Max number of characters of message content rendered by the `Debug` impl
/// before the output is truncated with an ellipsis.
const DEBUG_CONTENT_MAX_CHARS: usize = 50;

pub(crate) struct MessageInner {
    workspace_id: String,
    id: String,
    content: String,
    peer_id: String,
    session_id: String,
    metadata: HashMap<String, Value>,
    created_at: DateTime<Utc>,
    token_count: u64,
}

/// An enriched message in a Honcho workspace.
///
/// Wraps the raw [`MessageResponse`] with workspace context and provides
/// convenient field accessors.
#[derive(Clone)]
pub struct Message {
    inner: Arc<MessageInner>,
}

impl Message {
    /// Wraps a raw [`MessageResponse`] into a [`Message`].
    ///
    /// The workspace identity is taken directly from the server response
    /// (`resp.workspace_id`), which is authoritative.
    pub(crate) fn from_raw(resp: MessageResponse) -> Self {
        Self {
            inner: Arc::new(MessageInner {
                workspace_id: resp.workspace_id,
                id: resp.id,
                content: resp.content,
                peer_id: resp.peer_id,
                session_id: resp.session_id,
                metadata: resp.metadata,
                created_at: resp.created_at,
                token_count: resp.token_count,
            }),
        }
    }

    /// The message's unique identifier.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.inner.id
    }

    /// The message content text.
    #[must_use]
    pub fn content(&self) -> &str {
        &self.inner.content
    }

    /// ID of the peer that authored this message.
    #[must_use]
    pub fn peer_id(&self) -> &str {
        &self.inner.peer_id
    }

    /// ID of the session this message belongs to.
    #[must_use]
    pub fn session_id(&self) -> &str {
        &self.inner.session_id
    }

    /// Arbitrary key-value metadata attached to the message.
    #[must_use]
    pub fn metadata(&self) -> &HashMap<String, Value> {
        &self.inner.metadata
    }

    /// When this message was created.
    #[must_use]
    pub fn created_at(&self) -> &DateTime<Utc> {
        &self.inner.created_at
    }

    /// Token count for the message content.
    #[must_use]
    pub fn token_count(&self) -> u64 {
        self.inner.token_count
    }

    /// The workspace this message belongs to.
    #[must_use]
    pub fn workspace_id(&self) -> &str {
        &self.inner.workspace_id
    }
}

/// Zero-allocation `Debug` adapter that renders at most
/// [`DEBUG_CONTENT_MAX_CHARS`] characters of a string, appending an ellipsis
/// when the content is longer. Avoids the per-`Debug` `format!` allocation that
/// a `Cow::Owned` truncation would incur for long message content.
struct Truncated<'a>(&'a str);

impl fmt::Debug for Truncated<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0.char_indices().nth(DEBUG_CONTENT_MAX_CHARS) {
            // `escape_debug` is an iterator-based `Display`, so this writes the
            // quoted/escaped prefix directly into the formatter without
            // allocating an intermediate `String`.
            Some((idx, _)) => write!(f, "\"{}...\"", self.0[..idx].escape_debug()),
            None => fmt::Debug::fmt(self.0, f),
        }
    }
}

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Message")
            .field("id", &self.inner.id)
            .field("content", &Truncated(&self.inner.content))
            .field("peer_id", &self.inner.peer_id)
            .field("session_id", &self.inner.session_id)
            .finish_non_exhaustive()
    }
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.inner.content)
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
    use static_assertions::assert_impl_all;

    use super::*;

    assert_impl_all!(Message: Send, Sync, Clone, fmt::Debug, fmt::Display);

    fn fake_response() -> MessageResponse {
        MessageResponse {
            id: "msg_1".to_owned(),
            content: "hello world".to_owned(),
            peer_id: "peer_a".to_owned(),
            session_id: "sess_x".to_owned(),
            metadata: HashMap::new(),
            created_at: Utc::now(),
            workspace_id: "ws_1".to_owned(),
            token_count: 3,
        }
    }

    #[test]
    fn from_raw_maps_fields() {
        let resp = fake_response();
        let msg = Message::from_raw(resp);
        assert_eq!(msg.id(), "msg_1");
        assert_eq!(msg.content(), "hello world");
        assert_eq!(msg.peer_id(), "peer_a");
        assert_eq!(msg.session_id(), "sess_x");
        assert_eq!(msg.workspace_id(), "ws_1");
        assert_eq!(msg.token_count(), 3);
        assert!(msg.metadata().is_empty());
    }

    #[test]
    fn from_raw_stores_response_workspace_id() {
        // The workspace identity is taken from the server response.
        let mut resp = fake_response();
        resp.workspace_id = "ws_from_server".to_owned();
        let msg = Message::from_raw(resp);
        assert_eq!(msg.workspace_id(), "ws_from_server");
    }

    #[test]
    fn debug_truncates_long_content() {
        let mut resp = fake_response();
        resp.content = "a".repeat(80);
        let msg = Message::from_raw(resp);
        let dbg = format!("{msg:?}");
        assert!(dbg.contains("aaa..."));
        assert!(!dbg.contains(&"a".repeat(80)));
    }

    #[test]
    fn debug_short_content_not_truncated() {
        let resp = fake_response();
        let msg = Message::from_raw(resp);
        let dbg = format!("{msg:?}");
        assert!(dbg.contains("hello world"));
        assert!(!dbg.contains("..."));
    }

    #[test]
    fn display_returns_full_content() {
        let mut resp = fake_response();
        resp.content = "a".repeat(80);
        let msg = Message::from_raw(resp);
        assert_eq!(format!("{msg}"), "a".repeat(80));
    }

    #[test]
    fn debug_truncation_multibyte_utf8() {
        let mut resp = fake_response();
        resp.content = "\u{4e00}".repeat(60);
        let msg = Message::from_raw(resp);
        let dbg = format!("{msg:?}");
        assert!(dbg.contains("..."));
    }
}
