//! Message-related types for the Honcho API.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub use super::common::ReasoningConfiguration;

/// Raw API response for a message.
///
/// Represents a single message created by a peer within a session.
/// Use the top-level [`crate::Message`] wrapper for the enriched type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct MessageResponse {
    /// Unique message identifier.
    pub id: String,
    /// Message content text.
    pub content: String,
    /// ID of the peer that authored this message.
    pub peer_id: String,
    /// ID of the session this message belongs to.
    pub session_id: String,
    /// Arbitrary key-value metadata attached to the message.
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
    /// Timestamp when the message was created.
    pub created_at: DateTime<Utc>,
    /// ID of the workspace this message belongs to.
    pub workspace_id: String,
    /// Token count for the message content.
    #[serde(default)]
    pub token_count: u64,
}

/// Parameters for creating a single message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, bon::Builder)]
#[builder(on(String, into))]
#[builder(finish_fn = build)]
pub struct MessageCreate {
    /// Message content text (max 25 000 characters, server-validated).
    pub content: String,
    /// ID of the peer authoring the message.
    pub peer_id: String,
    /// Optional arbitrary metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    /// Optional message-level configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configuration: Option<MessageConfiguration>,
    /// Optional override for the creation timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
}

/// Parameters for batch-creating messages (1–100).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct MessageBatchCreate {
    /// List of messages to create.
    pub messages: Vec<MessageCreate>,
}

/// Parameters for updating a message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default, bon::Builder)]
#[builder(on(String, into))]
#[builder(finish_fn = build)]
pub struct MessageUpdate {
    /// Updated metadata (replaces existing).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

/// Request body for setting message metadata.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MessageMetadataSet {
    /// Metadata to set.
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Configuration that can be attached to a message.
///
/// All fields optional; message-level config overrides session and workspace config.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct MessageConfiguration {
    /// Reasoning configuration for this message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningConfiguration>,
}

/// Parameters for searching messages.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, bon::Builder)]
#[builder(on(String, into))]
#[builder(finish_fn = build)]
pub struct MessageSearchOptions {
    /// Search query string.
    pub query: String,
    /// Optional filters to scope the search.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<HashMap<String, serde_json::Value>>,
    /// Maximum number of results (1–100, default 10, server-validated).
    #[serde(default = "default_limit")]
    #[builder(default = default_limit())]
    pub limit: u32,
}

const fn default_limit() -> u32 {
    MessageSearchOptions::DEFAULT_LIMIT
}

impl MessageSearchOptions {
    const DEFAULT_LIMIT: u32 = 10;
}

/// Paginated response of [`MessageResponse`] items.
pub type MessagePage = super::pagination::Page<MessageResponse>;

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
    use super::*;

    #[test]
    fn message_search_options_default_limit() {
        let opts: MessageSearchOptions = serde_json::from_str(r#"{"query":"hello"}"#).unwrap();
        assert_eq!(opts.limit, 10);
    }

    #[test]
    fn message_response_token_count_defaults_to_zero_when_missing() {
        // Older servers may omit token_count; serde(default) must yield 0.
        let json = r#"{
            "id": "msg_01",
            "content": "hello",
            "peer_id": "peer_01",
            "session_id": "sess_01",
            "workspace_id": "ws_01",
            "created_at": "2024-01-01T00:00:00Z"
        }"#;
        let msg: MessageResponse = serde_json::from_str(json).unwrap();
        assert_eq!(msg.token_count, 0);
    }
}
