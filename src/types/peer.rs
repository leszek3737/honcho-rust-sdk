//! Peer-related types for the Honcho API.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub use super::common::PeerCardConfiguration;

/// A peer in a Honcho workspace.
///
/// Peers represent participants (users or agents) within a workspace.
/// Each peer has a unique ID scoped to its workspace.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Peer {
    /// Unique identifier for the peer.
    pub id: String,
    /// ID of the workspace this peer belongs to.
    pub workspace_id: String,
    /// Timestamp when the peer was created.
    pub created_at: DateTime<Utc>,
    /// Optional metadata attached to the peer.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
    /// Optional configuration for the peer.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub configuration: HashMap<String, serde_json::Value>,
}

/// Parameters for creating a new peer.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[builder(on(String, into))]
#[builder(finish_fn = build)]
pub struct PeerCreate {
    /// Unique identifier for the peer (alphanumeric, hyphens, underscores).
    pub id: String,
    /// Optional metadata to attach to the peer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    /// Optional configuration for the peer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<HashMap<String, serde_json::Value>>,
}

/// Parameters for updating an existing peer.
///
/// # Warning: no-op when both fields are `None`
///
/// If both `metadata` and `configuration` are `None`, the request body
/// serializes to `{}`. The server will either perform a silent no-op or
/// return an error. Ensure at least one field is `Some` before sending.
/// Compile-time enforcement of this constraint is deferred to a future major
/// release.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[builder(on(String, into))]
#[builder(finish_fn = build)]
pub struct PeerUpdate {
    /// Updated metadata. If provided, replaces the existing metadata entirely.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    /// Updated configuration. If provided, replaces the existing configuration entirely.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<HashMap<String, serde_json::Value>>,
}

/// Filter parameters for listing peers.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[builder(on(String, into))]
#[builder(finish_fn = build)]
pub struct PeerGet {
    /// Optional metadata-based filters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filters: Option<HashMap<String, serde_json::Value>>,
}

/// Request body for setting peer metadata.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PeerMetadataSet {
    /// Metadata to set.
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Typed peer configuration with known fields.
///
/// Used as the typed view over the peer configuration map. The server may
/// return additional fields not captured here; use the `_raw` escape hatches
/// on [`crate::Peer`] to access them.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PeerConfig {
    /// Whether Honcho will use reasoning to form a representation of this peer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observe_me: Option<bool>,
    /// Whether this peer should form representations of other peers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observe_others: Option<bool>,
}

/// Request body for setting peer configuration.
///
/// # Warning: full-replace (PUT) semantics
///
/// This maps to a **PUT** endpoint. The server replaces the **entire** peer
/// configuration object with the value of `configuration`. Any keys present
/// server-side but absent from [`PeerConfig`] (e.g. fields added in future
/// server versions) will be **silently wiped**.
///
/// To avoid accidental data loss, read the current configuration first and
/// merge the desired changes before calling
/// [`set_configuration`](crate::Peer::set_configuration).
/// Merge / read-modify-write helpers are deferred to a future major release.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PeerConfigurationSet {
    /// Configuration to set.
    pub configuration: PeerConfig,
}

/// Response from getting a peer card.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerCardResponse {
    /// The peer card content lines, or `None` if no card exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_card: Option<Vec<String>>,
}

/// Request body for setting a peer card.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, bon::Builder)]
#[builder(on(String, into))]
#[builder(finish_fn = build)]
pub struct PeerCardSet {
    /// The peer card content lines to set.
    #[builder(into)]
    pub peer_card: Vec<String>,
}

/// Context for a peer, combining representation and peer card.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerContext {
    /// The ID of the observer peer.
    pub peer_id: String,
    /// The ID of the target peer being observed.
    pub target_id: String,
    /// Curated subset of the representation of the target from the observer's perspective.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub representation: Option<String>,
    /// The peer card for the target from the observer's perspective.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_card: Option<Vec<String>>,
}

/// Parameters for getting a peer's representation.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[builder(on(String, into))]
#[builder(finish_fn = build)]
pub struct PeerRepresentationGet {
    /// Optional session ID to scope the representation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Optional target peer ID to get the representation for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Optional query to curate the representation around semantic search results.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_query: Option<String>,
    /// Number of semantic-search-retrieved conclusions to include.
    /// Only used if `search_query` is provided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_top_k: Option<u32>,
    /// Maximum distance for semantically relevant conclusions.
    /// Only used if `search_query` is provided.
    ///
    /// Server-validated; range 0.0–1.0. Values outside this range (including
    /// negative values, values above `1.0`, or `NaN`) are accepted by this
    /// type but will be rejected by the server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_max_distance: Option<f64>,
    /// Whether to include the most frequent conclusions.
    /// Only used if `search_query` is provided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_most_frequent: Option<bool>,
    /// Maximum number of conclusions to include in the representation.
    /// Only used if `search_query` is provided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_conclusions: Option<u32>,
}

/// Options for `Peer::context_with_options`.
///
/// # Note: floating-point equality
///
/// This type derives [`PartialEq`] but contains `search_max_distance:
/// Option<f64>`. Floating-point `NaN` is never equal to itself
/// (`NaN != NaN`), so two instances that both carry `NaN` as the distance
/// will compare as unequal. Avoid relying on `==` when the distance field
/// may hold `NaN`; write a custom comparator if structural equality is
/// required. Deriving `Eq` requires a validated newtype and is deferred
/// to a future major release.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, bon::Builder)]
#[builder(on(String, into))]
#[builder(finish_fn = build)]
pub struct PeerContextOptions {
    /// Target peer to get context for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Semantic search query to filter relevant conclusions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_query: Option<String>,
    /// Number of semantically relevant facts to return.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_top_k: Option<u32>,
    /// Maximum semantic distance for search results.
    ///
    /// Server-validated; range 0.0–1.0. Values outside this range (including
    /// negative values, values above `1.0`, or `NaN`) are accepted by this
    /// type but will be rejected by the server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_max_distance: Option<f64>,
    /// Whether to include the most frequent conclusions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_most_frequent: Option<bool>,
    /// Maximum number of conclusions to include.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_conclusions: Option<u32>,
}

/// A page of [`Peer`] results from a paginated list endpoint.
pub type PeerPage = super::pagination::Page<Peer>;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn peer_config_is_eq() {
        let a = PeerConfig {
            observe_me: Some(true),
            observe_others: None,
        };
        assert_eq!(a, a.clone());
    }

    #[test]
    fn peer_configuration_set_is_eq() {
        let a = PeerConfigurationSet {
            configuration: PeerConfig::default(),
        };
        assert_eq!(a, a.clone());
    }

    #[test]
    fn peer_card_response_is_eq() {
        let a = PeerCardResponse {
            peer_card: Some(vec!["line".to_string()]),
        };
        assert_eq!(a, a.clone());
    }

    #[test]
    fn peer_card_set_is_eq() {
        let a = PeerCardSet {
            peer_card: vec!["line".to_string()],
        };
        assert_eq!(a, a.clone());
    }

    #[test]
    fn peer_context_is_eq() {
        let a = PeerContext {
            peer_id: "p1".to_string(),
            target_id: "p2".to_string(),
            representation: None,
            peer_card: None,
        };
        assert_eq!(a, a.clone());
    }
}
