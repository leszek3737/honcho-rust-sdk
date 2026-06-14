//! Workspace domain types.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub use super::common::{
    DreamConfiguration, PeerCardConfiguration, ReasoningConfiguration, SummaryConfiguration,
};

/// A workspace resource.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Workspace {
    /// Unique identifier for the workspace.
    pub id: String,
    /// Arbitrary metadata attached to the workspace.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
    /// Workspace-level configuration overrides.
    #[serde(default)]
    pub configuration: WorkspaceConfiguration,
    /// When the workspace was created.
    pub created_at: DateTime<Utc>,
}

/// The set of options that can be in a workspace-level configuration dictionary.
///
/// All fields are optional. Session-level configuration overrides workspace-level
/// configuration, which overrides global configuration.
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct WorkspaceConfiguration {
    /// Configuration for reasoning functionality.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningConfiguration>,
    /// Configuration for peer card functionality.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer_card: Option<PeerCardConfiguration>,
    /// Configuration for summary functionality.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<SummaryConfiguration>,
    /// Configuration for dream functionality.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dream: Option<DreamConfiguration>,
}

/// Request body for creating a workspace.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, bon::Builder)]
#[builder(on(String, into))]
#[builder(finish_fn = build)]
pub struct WorkspaceCreate {
    /// Unique identifier for the new workspace (1-512 chars, `[a-zA-Z0-9_-]+`).
    pub id: String,
    /// Arbitrary metadata. Defaults to `{}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    /// Workspace-level configuration overrides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<WorkspaceConfiguration>,
}

/// Request body for updating a workspace.
///
/// Both fields use three-state semantics matching the `OpenAPI` contract
/// (each property is nullable and non-required):
/// - field omitted (`None`) — leave the value unchanged on the server;
/// - explicit `null` (`Some(None)`) — clear/reset the value on the server;
/// - explicit value (`Some(Some(_))`) — overwrite with the new value.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, bon::Builder)]
#[builder(on(String, into))]
#[builder(finish_fn = build)]
pub struct WorkspaceUpdate {
    /// Updated metadata. Omitted = leave unchanged; explicit `null` = clear.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "double_option"
    )]
    pub metadata: Option<Option<HashMap<String, serde_json::Value>>>,
    /// Updated configuration. Omitted = leave unchanged; explicit `null` = clear.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "double_option"
    )]
    pub configuration: Option<Option<WorkspaceConfiguration>>,
}

/// Request body for listing/getting workspaces.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, bon::Builder)]
#[builder(on(String, into))]
#[builder(finish_fn = build)]
pub struct WorkspaceGet {
    /// Optional metadata filters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<HashMap<String, serde_json::Value>>,
}

/// Request body for setting workspace metadata.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceMetadataSet {
    /// Metadata to set.
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Request body for setting workspace configuration.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceConfigurationSet {
    /// Configuration to set.
    pub configuration: serde_json::Value,
}

/// Request body for workspace search.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceSearchRequest {
    /// Search query string.
    pub query: String,
    /// Maximum number of results. Omitted lets the server choose its default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// Optional metadata-based filters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<HashMap<String, serde_json::Value>>,
}

/// A page of workspace results.
pub type WorkspacePage = super::pagination::Page<Workspace>;

/// Serde helper for `Option<Option<T>>` fields that must distinguish an
/// omitted field (outer `None`, skipped on the wire) from an explicit JSON
/// `null` (`Some(None)`). Referenced via `#[serde(with = "double_option")]`.
///
/// Without this, an explicit `null` deserializes to the outer `None`, making
/// the round-trip asymmetric with serialization (where `Some(None)` emits
/// `null`).
mod double_option {
    // serde's `with` contract dictates these exact signatures
    // (`&Option<Option<T>>` / `-> Option<Option<T>>`), so these lints are
    // unavoidable here rather than a design smell.
    #![allow(clippy::option_option, clippy::ref_option)]
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub(super) fn serialize<T, S>(v: &Option<Option<T>>, s: S) -> Result<S::Ok, S::Error>
    where
        T: Serialize,
        S: Serializer,
    {
        match v {
            // `Some(Some(_))` -> value, `Some(None)` -> JSON null.
            Some(inner) => inner.serialize(s),
            // Outer `None` is skipped via `skip_serializing_if`; serialize as
            // null defensively if reached directly.
            None => s.serialize_none(),
        }
    }

    pub(super) fn deserialize<'de, T, D>(d: D) -> Result<Option<Option<T>>, D::Error>
    where
        T: Deserialize<'de>,
        D: Deserializer<'de>,
    {
        // A present field (value or null) maps to `Some(_)`; an absent field
        // is handled by `#[serde(default)]` -> outer `None`.
        Option::<T>::deserialize(d).map(Some)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn workspace_update_explicit_null_metadata_roundtrips() {
        let body = WorkspaceUpdate {
            metadata: Some(None),
            configuration: None,
        };

        // `Some(None)` must serialize to an explicit JSON `null` (clear),
        // while the omitted `configuration` field is skipped entirely.
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json, serde_json::json!({ "metadata": null }));

        // The explicit `null` must deserialize back to `Some(None)`, not the
        // outer `None` — proving round-trip symmetry.
        let back: WorkspaceUpdate = serde_json::from_value(json).unwrap();
        assert_eq!(back.metadata, Some(None));
        assert_eq!(back.configuration, None);
    }

    #[test]
    fn workspace_update_omitted_metadata_is_outer_none() {
        // An omitted field deserializes to the outer `None` (leave unchanged).
        let back: WorkspaceUpdate = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(back.metadata, None);
        assert_eq!(back.configuration, None);
    }

    #[test]
    fn workspace_search_request_omits_absent_limit() {
        let body = WorkspaceSearchRequest {
            query: "q".to_owned(),
            limit: None,
            filters: None,
        };
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json, serde_json::json!({ "query": "q" }));
    }
}
