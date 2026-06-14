//! Conclusion types for the Honcho API.
//!
//! Maps the `OpenAPI` schemas: `Conclusion`, `ConclusionCreate`,
//! `ConclusionBatchCreate`, `ConclusionGet`, `ConclusionQuery`, `Page[Conclusion]`.

use crate::types::pagination::Page;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A conclusion about a peer, produced by observation.
///
/// Maps `OpenAPI` `Conclusion`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct ConclusionResponse {
    /// Unique identifier.
    pub id: String,
    /// The conclusion content text.
    pub content: String,
    /// The peer who made the conclusion.
    pub observer_id: String,
    /// The peer the conclusion is about.
    pub observed_id: String,
    /// Optional session ID the conclusion belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// When the conclusion was created.
    pub created_at: DateTime<Utc>,
}

/// Request body for creating a single conclusion.
///
/// Maps `OpenAPI` `ConclusionCreate`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, bon::Builder)]
#[builder(on(String, into))]
#[builder(finish_fn = build)]
#[non_exhaustive]
pub struct ConclusionCreate {
    /// The conclusion content (1–65535 chars).
    pub content: String,
    /// The peer making the conclusion.
    pub observer_id: String,
    /// The peer the conclusion is about.
    pub observed_id: String,
    /// Optional session ID to associate the conclusion with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// Request body for batch-creating conclusions (1–100 items).
///
/// Maps `OpenAPI` `ConclusionBatchCreate`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, bon::Builder)]
#[builder(finish_fn = build)]
#[non_exhaustive]
pub struct ConclusionBatchCreate {
    /// The conclusions to create.
    pub conclusions: Vec<ConclusionCreate>,
}

/// Typed filters for conclusion list and query requests.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default, bon::Builder)]
#[builder(on(String, into))]
#[builder(finish_fn = build)]
#[non_exhaustive]
pub struct ConclusionFilters {
    /// Filter by observer peer ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observer_id: Option<String>,
    /// Filter by observed peer ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_id: Option<String>,
    /// Optional session ID filter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// Request body for listing conclusions with optional filters.
///
/// Maps `OpenAPI` `ConclusionGet`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default, bon::Builder)]
#[builder(finish_fn = build)]
#[non_exhaustive]
pub struct ConclusionGet {
    /// Optional metadata filters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<ConclusionFilters>,
}

/// Request body for semantic search over conclusions.
///
/// Maps `OpenAPI` `ConclusionQuery`.
///
/// # Equality caveat
///
/// The derived [`PartialEq`] compares `distance` via raw `f64` equality, which
/// is **not reflexive for `NaN`**: a value whose `distance` is `Some(f64::NAN)`
/// is *not* equal to itself. For this reason the type intentionally does **not**
/// implement [`Eq`]. A validated `Distance` newtype constrained to `[0.0, 1.0]`
/// (which excludes `NaN`) is the proper fix and is deferred to a later change.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, bon::Builder)]
#[builder(on(String, into))]
#[builder(finish_fn = build)]
#[non_exhaustive]
pub struct ConclusionQuery {
    /// Semantic search query string.
    pub query: String,
    /// Number of results to return (1–100, default 10).
    ///
    /// Always serialized: an explicitly set value (even one equal to the
    /// default) is sent on the wire, so the server never has to infer intent
    /// from omission.
    #[serde(default = "default_top_k")]
    #[builder(default = DEFAULT_TOP_K)]
    pub top_k: u32,
    /// Maximum cosine distance threshold (0.0–1.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance: Option<f64>,
    /// Additional metadata filters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<ConclusionFilters>,
}

/// Default number of semantic-search results requested by [`ConclusionQuery`].
const DEFAULT_TOP_K: u32 = 10;

fn default_top_k() -> u32 {
    DEFAULT_TOP_K
}

/// A page of conclusion results.
///
/// Alias for `Page<ConclusionResponse>`, maps `OpenAPI` `Page[Conclusion]`.
pub type ConclusionPage = Page<ConclusionResponse>;

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

    use super::*;

    #[test]
    fn top_k_serializes_even_when_equal_to_default() {
        // The builder default must match the documented default.
        let query = ConclusionQuery::builder().query("hello").build();
        assert_eq!(query.top_k, DEFAULT_TOP_K);

        // And the default value must still appear on the wire (no magic-value
        // skip predicate) so the server never has to infer intent from omission.
        let value = serde_json::to_value(&query).unwrap();
        assert_eq!(value["top_k"], serde_json::json!(DEFAULT_TOP_K));
    }

    #[test]
    fn explicit_top_k_round_trips() {
        let query = ConclusionQuery::builder().query("hello").top_k(5).build();
        let value = serde_json::to_value(&query).unwrap();
        assert_eq!(value["top_k"], serde_json::json!(5));

        let decoded: ConclusionQuery = serde_json::from_value(value).unwrap();
        assert_eq!(decoded.top_k, 5);
    }

    #[test]
    fn missing_top_k_deserializes_to_default() {
        // serde `default` keeps deserialization tolerant of an absent field.
        let decoded: ConclusionQuery =
            serde_json::from_value(serde_json::json!({ "query": "hi" })).unwrap();
        assert_eq!(decoded.top_k, DEFAULT_TOP_K);
    }

    #[test]
    fn conclusion_filters_default_is_all_none() {
        let filters = ConclusionFilters::default();
        assert_eq!(filters, ConclusionFilters::builder().build());
        assert!(filters.observer_id.is_none());
        assert!(filters.observed_id.is_none());
        assert!(filters.session_id.is_none());
    }
}
