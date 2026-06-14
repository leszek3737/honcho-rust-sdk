//! `FastAPI` validation error types.
//!
//! These types model the [`HTTPValidationError`] and [`ValidationError`] schemas
//! that `FastAPI` returns for request validation failures (HTTP 422). They are
//! standalone types usable alongside [`crate::error::HonchoError`] for
//! inspecting validation details from the `body` field of
//! [`crate::error::HonchoError::UnprocessableEntity`].
//!
//! The `detail` payload is modeled by the untagged [`Detail`] enum because
//! `FastAPI` returns it either as a structured array of [`ValidationError`]
//! values or — when raised via `HTTPException(422, detail="...")` — as a plain
//! string. Use [`HTTPValidationError::errors`] / [`HTTPValidationError::message`]
//! to inspect either shape without matching on [`Detail`] directly.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A single validation error from `FastAPI`.
///
/// Corresponds to the `ValidationError` schema in the `OpenAPI` spec.
///
/// Required fields: `loc`, `msg`, `type`.
/// Optional fields: `input`, `ctx`.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ValidationError {
    /// Location of the error as a path through the request structure.
    ///
    /// Elements are either strings (field/object names) or integers (array indices).
    pub loc: Vec<LocationSegment>,
    /// Human-readable error message.
    pub msg: String,
    /// Machine-readable error type identifier (e.g. `"value_error.missing"`).
    #[serde(rename = "type")]
    pub error_type: String,
    /// The input value that failed validation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<serde_json::Value>,
    /// Optional context providing additional details about the error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ctx: Option<serde_json::Map<String, serde_json::Value>>,
}

impl ValidationError {
    /// Renders [`Self::loc`] as a dotted/indexed path, e.g. `"body.items[0].name"`.
    ///
    /// String (and other non-integer) segments are joined with `.`; integer
    /// segments are rendered as `[index]` and attached without a separator.
    #[must_use]
    pub fn loc_path(&self) -> String {
        let mut path = String::new();
        for segment in &self.loc {
            match segment {
                LocationSegment::Integer(index) => {
                    path.push('[');
                    path.push_str(&index.to_string());
                    path.push(']');
                }
                other => {
                    if !path.is_empty() {
                        path.push('.');
                    }
                    path.push_str(&other.to_string());
                }
            }
        }
        path
    }
}

/// A segment within a validation error location path.
///
/// `FastAPI` location paths contain either string keys (field names) or integer
/// indices (array positions). This enum captures both, with [`Self::Other`] as a
/// forward-compatible catch-all so a single unexpected `loc` element (null,
/// float, bool, ...) does not fail deserialization of the whole error body.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum LocationSegment {
    /// A string field or key name.
    String(String),
    /// An integer array index.
    Integer(i64),
    /// Any other JSON value `FastAPI` may emit in a `loc` element.
    ///
    /// Kept last so the specific [`Self::String`] / [`Self::Integer`] variants
    /// are tried first by the untagged deserializer.
    Other(serde_json::Value),
}

impl fmt::Display for LocationSegment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::String(value) => f.write_str(value),
            Self::Integer(value) => write!(f, "{value}"),
            Self::Other(value) => write!(f, "{value}"),
        }
    }
}

/// The `detail` payload of an [`HTTPValidationError`].
///
/// `FastAPI` returns validation `detail` either as a structured array of
/// [`ValidationError`] values, or — when raised via
/// `HTTPException(422, detail="...")` — as a plain string message. This untagged
/// enum accepts both shapes.
///
/// The [`Self::Errors`] variant is listed first so JSON arrays match the
/// structured form; [`Self::Message`] captures the string form.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Detail {
    /// Structured list of validation errors (the common `FastAPI` shape).
    Errors(Vec<ValidationError>),
    /// A plain string message (e.g. from `HTTPException(422, detail="...")`).
    Message(String),
}

impl Default for Detail {
    fn default() -> Self {
        Self::Errors(Vec::new())
    }
}

/// HTTP-level wrapper for `FastAPI` validation errors.
///
/// Corresponds to the `HTTPValidationError` schema in the `OpenAPI` spec.
/// Returned as the response body for HTTP 422 responses.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HTTPValidationError {
    /// Validation detail: either structured errors or a plain message.
    ///
    /// Per the `OpenAPI` spec `detail` is not required, so an absent field
    /// defaults to an empty [`Detail::Errors`] list.
    #[serde(default)]
    pub detail: Detail,
}

impl HTTPValidationError {
    /// Returns the structured validation errors.
    ///
    /// Yields the error slice when [`Self::detail`] is [`Detail::Errors`], or an
    /// empty slice when it is a plain [`Detail::Message`].
    #[must_use]
    pub fn errors(&self) -> &[ValidationError] {
        match &self.detail {
            Detail::Errors(errors) => errors,
            Detail::Message(_) => &[],
        }
    }

    /// Returns the plain message when [`Self::detail`] is [`Detail::Message`].
    ///
    /// Yields [`None`] when the detail is a structured [`Detail::Errors`] list.
    #[must_use]
    pub fn message(&self) -> Option<&str> {
        match &self.detail {
            Detail::Message(message) => Some(message),
            Detail::Errors(_) => None,
        }
    }
}
