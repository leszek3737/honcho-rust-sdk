//! Dialectic API types — chat/query with representation-backed responses.

use serde::{Deserialize, Serialize};

use crate::error::{HonchoError, Result};

const MAX_DIALECTIC_QUERY_CHARS: usize = 10_000;

/// Error message for queries exceeding [`MAX_DIALECTIC_QUERY_CHARS`].
///
/// Kept as a `&'static str` so the validation error path neither runs the
/// `format!` machinery nor embeds a runtime-formatted number.
const QUERY_TOO_LONG_MSG: &str = "query must be at most 10000 characters";

/// Compile-time guard keeping [`QUERY_TOO_LONG_MSG`] in sync with the limit:
/// if the constant changes, this assertion fails and forces the message update.
const _: () = assert!(MAX_DIALECTIC_QUERY_CHARS == 10_000);

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
    /// Unknown / future server-side variant not modeled by this SDK version.
    ///
    /// Forward-compatibility catch-all: deserializing an unrecognized
    /// `reasoning_level` string yields this variant instead of a hard error.
    /// `#[serde(other)]` only affects deserialization; serializing `Unknown`
    /// is a degenerate path (emits `"unknown"`) and is not expected in
    /// outbound requests.
    #[serde(other)]
    Unknown,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_default_reasoning_level(level: &ReasoningLevel) -> bool {
    // Reference the derived default so changing `#[default]` cannot desync
    // serialization from the builder/struct default.
    *level == ReasoningLevel::default()
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_default_bool(val: &bool) -> bool {
    !val
}

/// Options for a dialectic chat request.
///
/// Maps `DialecticOptions` from the `OpenAPI` spec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
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
    ///
    /// # Warning
    ///
    /// This flag only sets the request payload field; it does not by itself
    /// switch the client onto the streaming transport. Setting `stream(true)`
    /// while sending via the plain JSON `POST` (non-streaming) path will not
    /// produce a streamed response — use the dedicated streaming entry points
    /// (e.g. `chat_stream`) instead. This interaction is a known rough edge.
    #[serde(default, skip_serializing_if = "is_default_bool")]
    #[builder(default = false)]
    pub stream: bool,
    /// Level of reasoning to apply.
    #[serde(default, skip_serializing_if = "is_default_reasoning_level")]
    #[builder(default)]
    pub reasoning_level: ReasoningLevel,
}

/// Validate a dialectic query before sending it to the API.
pub fn validate_dialectic_query(query: &str) -> Result<()> {
    // Reject whitespace-only queries, not just the empty string.
    if query.trim().is_empty() {
        return Err(HonchoError::Validation(
            "query must not be empty".to_owned(),
        ));
    }

    // Short-circuit: probe the char at the over-limit index instead of
    // counting the whole string (which would scan every char).
    if query.chars().nth(MAX_DIALECTIC_QUERY_CHARS).is_some() {
        return Err(HonchoError::Validation(QUERY_TOO_LONG_MSG.to_owned()));
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
    ///     .build();
    /// opts.validate()?;
    /// ```
    pub fn validate(&self) -> Result<()> {
        validate_dialectic_query(&self.query)
    }
}

/// Response from the representation endpoint.
///
/// Maps `RepresentationResponse` from the `OpenAPI` spec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RepresentationResponse {
    /// The peer representation text.
    pub representation: String,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

    use super::*;
    use serde_json::json;

    #[test]
    fn reasoning_level_unknown_string_deserializes_to_unknown() {
        // A future/unrecognized server variant must deserialize to `Unknown`,
        // not produce a hard error.
        let parsed: ReasoningLevel = serde_json::from_value(json!("some_future_variant")).unwrap();
        assert_eq!(parsed, ReasoningLevel::Unknown);
    }

    #[test]
    fn known_reasoning_levels_still_deserialize() {
        // Adding the catch-all must not shadow the explicit variants.
        let low: ReasoningLevel = serde_json::from_value(json!("low")).unwrap();
        let max: ReasoningLevel = serde_json::from_value(json!("max")).unwrap();
        assert_eq!(low, ReasoningLevel::Low);
        assert_eq!(max, ReasoningLevel::Max);
    }

    #[test]
    fn dialectic_payload_with_unknown_reasoning_level_deserializes() {
        // A payload carrying an unknown `reasoning_level` parses successfully,
        // mapping the field to `Unknown`.
        let parsed: DialecticOptions = serde_json::from_value(json!({
            "query": "hello",
            "reasoning_level": "ultra_max_2099"
        }))
        .unwrap();
        assert_eq!(parsed.query, "hello");
        assert_eq!(parsed.reasoning_level, ReasoningLevel::Unknown);
    }

    #[test]
    fn whitespace_only_query_is_rejected() {
        assert!(validate_dialectic_query("   \t\n ").is_err());
        assert!(validate_dialectic_query("").is_err());
        assert!(validate_dialectic_query("ok").is_ok());
    }
}
