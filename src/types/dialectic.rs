//! Dialectic API types — chat/query with representation-backed responses.

use serde::{Deserialize, Serialize};

use crate::error::{HonchoError, Result};

const MAX_DIALECTIC_QUERY_CHARS: usize = 10_000;

/// Reasoning effort level for dialectic queries.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningLevel {
    /// Minimal reasoning.
    Minimal,
    /// Low reasoning (default).
    #[default]
    Low,
    /// Medium reasoning.
    Medium,
    /// High reasoning.
    High,
    /// Maximum reasoning.
    Max,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_default_reasoning_level(level: &ReasoningLevel) -> bool {
    matches!(level, ReasoningLevel::Low)
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_default_bool(val: &bool) -> bool {
    !val
}

/// Options for a dialectic chat request.
///
/// Maps `DialecticOptions` from the `OpenAPI` spec.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, bon::Builder)]
#[non_exhaustive]
#[builder(derive(Debug), on(String, into))]
#[builder(finish_fn = build)]
pub struct DialecticOptions {
    /// Dialectic API prompt (1–10,000 chars).
    pub query: String,
    /// ID of the session to scope the representation to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Optional peer to get the representation for, from the perspective of this peer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Whether to stream the response.
    #[serde(default, skip_serializing_if = "is_default_bool")]
    #[builder(default = false)]
    pub stream: bool,
    /// Level of reasoning to apply.
    #[serde(default, skip_serializing_if = "is_default_reasoning_level")]
    #[builder(default = ReasoningLevel::Low)]
    pub reasoning_level: ReasoningLevel,
}

/// Validate a dialectic query before sending it to the API.
pub fn validate_dialectic_query(query: &str) -> Result<()> {
    if query.is_empty() {
        return Err(HonchoError::Validation(
            "query must not be empty".to_owned(),
        ));
    }

    if query.chars().count() > MAX_DIALECTIC_QUERY_CHARS {
        return Err(HonchoError::Validation(format!(
            "query must be at most {MAX_DIALECTIC_QUERY_CHARS} characters"
        )));
    }

    Ok(())
}

impl DialecticOptions {
    /// Validate options before sending them to the API.
    ///
    /// This is a separate method (not part of the builder's `build()`) because
    /// `bon::Builder` with `finish_fn = build` does not support fallible finish.
    /// Call this after `.build()` and before passing the options to the API:
    ///
    /// ```ignore
    /// let opts = DialecticOptions::builder()
    ///     .query("hello")
    ///     .build()
    ///     .validate()?;
    /// ```
    pub fn validate(&self) -> Result<()> {
        validate_dialectic_query(&self.query)
    }
}

/// Response from the representation endpoint.
///
/// Maps `RepresentationResponse` from the `OpenAPI` spec.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RepresentationResponse {
    /// The peer representation text.
    pub representation: String,
}
