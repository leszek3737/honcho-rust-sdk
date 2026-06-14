//! Common types shared across domain modules.

use serde::{Deserialize, Serialize};

/// JSON value alias for metadata and configuration fields.
pub type JsonValue = serde_json::Value;

/// Configuration for reasoning functionality.
///
/// # Examples
///
/// This type is `#[non_exhaustive]`, so it cannot be built with a struct
/// literal (and therefore not with functional-update `..Default::default()`
/// syntax) from outside the crate. Start from [`Default`] and set the fields
/// you need:
///
/// ```
/// use honcho_ai::types::common::ReasoningConfiguration;
///
/// let mut config = ReasoningConfiguration::default();
/// config.enabled = Some(true);
/// config.custom_instructions = Some("focus on the most recent turns".to_owned());
///
/// assert_eq!(config.enabled, Some(true));
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningConfiguration {
    /// Whether to enable reasoning functionality.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Custom instructions for the reasoning system.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_instructions: Option<String>,
}

/// Configuration for automatic session summarization.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SummaryConfiguration {
    /// Whether to enable summary functionality.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Number of messages per short summary (minimum 10, server-validated).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages_per_short_summary: Option<u32>,
    /// Number of messages per long summary (minimum 20, server-validated).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages_per_long_summary: Option<u32>,
}

/// Configuration for dream functionality.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DreamConfiguration {
    /// Whether to enable dream functionality.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

/// Configuration for peer card generation and usage.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerCardConfiguration {
    /// Whether to use the peer card during the reasoning process.
    #[serde(rename = "use", skip_serializing_if = "Option::is_none")]
    pub use_peer_card: Option<bool>,
    /// Whether to generate a peer card based on content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create: Option<bool>,
}
