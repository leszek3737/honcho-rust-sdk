//! Conclusion types for the Honcho API.
//!
//! Maps the `OpenAPI` schemas: `Conclusion`, `ConclusionCreate`,
//! `ConclusionBatchCreate`, `ConclusionGet`, `ConclusionQuery`, `Page[Conclusion]`.

use std::collections::HashMap;

use crate::types::pagination::Page;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Reasoning level of a conclusion.
///
/// Maps the upstream `ConclusionLevel`. `"explicit"` conclusions are extracted
/// directly from messages; the other variants are derived during dreaming.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConclusionLevel {
    /// Extracted directly from a message.
    #[default]
    Explicit,
    /// Derived during dreaming.
    Deductive,
    /// Derived during dreaming.
    Inductive,
    /// Derived during dreaming; records a contradiction.
    Contradiction,
    /// Unknown / future server-side variant not modeled by this SDK version.
    ///
    /// Forward-compatibility catch-all: deserializing an unrecognized
    /// `level` string yields this variant instead of a hard error.
    /// `#[serde(other)]` only affects deserialization; serializing `Unknown`
    /// is a degenerate path (emits `"unknown"`) and is not expected in
    /// outbound requests.
    #[serde(other)]
    Unknown,
}

impl std::fmt::Display for ConclusionLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Explicit => "explicit",
            Self::Deductive => "deductive",
            Self::Inductive => "inductive",
            Self::Contradiction => "contradiction",
            Self::Unknown => "unknown",
        })
    }
}

/// A conclusion about a peer, produced by observation.
///
/// Maps `OpenAPI` `Conclusion`. The `level` field is not yet declared in the
/// shipped `OpenAPI` spec but is always sent by the server (older servers
/// predate the field; `#[serde(default)]` keeps deserialization tolerant).
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
    /// Reasoning level. Defaults to [`ConclusionLevel::Explicit`] when absent
    /// (older servers).
    #[serde(default)]
    pub level: ConclusionLevel,
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

/// Request body for listing conclusions with optional filters.
///
/// Maps `OpenAPI` `ConclusionGet`. The `filters` map is free-form
/// (`additionalProperties: true` in the spec); the scope-managed keys
/// `observer_id` / `observed_id` / `session_id` are injected by the builders
/// on [`crate::ConclusionScope`], and callers can add arbitrary extra keys
/// (e.g. `"level"`) via
/// [`ListConclusionsBuilder::filters`](crate::conclusion::ListConclusionsBuilder::filters).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default, bon::Builder)]
#[builder(finish_fn = build)]
#[non_exhaustive]
pub struct ConclusionGet {
    /// Optional free-form filter criteria.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filters: Option<HashMap<String, serde_json::Value>>,
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
    /// Optional free-form filter criteria. The scope-managed keys
    /// `observer_id` / `observed_id` are injected by
    /// [`QueryConclusionsBuilder`](crate::conclusion::QueryConclusionsBuilder);
    /// unlike [`ConclusionGet`], this builder has no `.session()` method, so
    /// `session_id` is a legitimate caller-supplied filter here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filters: Option<HashMap<String, serde_json::Value>>,
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
    fn conclusion_response_level_defaults_to_explicit_when_absent() {
        let decoded: ConclusionResponse = serde_json::from_value(serde_json::json!({
            "id": "c1",
            "content": "x",
            "observer_id": "o",
            "observed_id": "d",
            "created_at": "2025-01-01T00:00:00Z",
        }))
        .unwrap();
        assert_eq!(decoded.level, ConclusionLevel::Explicit);

        // Re-serializing emits the default value (`#[serde(default)]` has no
        // skip predicate), so the field round-trips to `"explicit"`.
        let v = serde_json::to_value(&decoded).unwrap();
        assert_eq!(v["level"], serde_json::json!("explicit"));
    }

    #[test]
    fn conclusion_response_level_round_trips_each_variant() {
        for s in ["explicit", "deductive", "inductive", "contradiction"] {
            let decoded: ConclusionLevel = serde_json::from_value(serde_json::json!(s)).unwrap();
            let re = serde_json::to_value(decoded).unwrap();
            assert_eq!(
                re,
                serde_json::json!(s),
                "level variant {s} must round-trip"
            );
        }
    }

    #[test]
    fn conclusion_level_unknown_variant_does_not_error() {
        // Forward-compatibility: a server-side addition of a new level variant
        // (e.g. `"abductive"`) must not break deserialization of the entire
        // response. `#[serde(other)]` maps any unrecognized string to `Unknown`.
        let decoded: ConclusionLevel =
            serde_json::from_value(serde_json::json!("abductive")).unwrap();
        assert_eq!(decoded, ConclusionLevel::Unknown);

        // Serializing `Unknown` emits the degenerate `"unknown"` string.
        let re = serde_json::to_value(decoded).unwrap();
        assert_eq!(re, serde_json::json!("unknown"));
    }

    #[test]
    fn conclusion_level_display() {
        assert_eq!(ConclusionLevel::Explicit.to_string(), "explicit");
        assert_eq!(ConclusionLevel::Deductive.to_string(), "deductive");
        assert_eq!(ConclusionLevel::Inductive.to_string(), "inductive");
        assert_eq!(ConclusionLevel::Contradiction.to_string(), "contradiction");
        assert_eq!(ConclusionLevel::Unknown.to_string(), "unknown");
    }
}
