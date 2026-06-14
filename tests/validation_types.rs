#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

mod common;

use common::{load_fixture, roundtrip, validate_openapi};
use honcho_ai::types::validation::{Detail, HTTPValidationError, LocationSegment, ValidationError};

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::json;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Deserializes `tests/fixtures/<name>/<variant>.json` into `T`.
///
/// Collapses the load-fixture + `from_value` boilerplate repeated across the
/// per-field assertion tests below.
fn deserialize_fixture<T: DeserializeOwned>(name: &str, variant: &str) -> T {
    serde_json::from_value(load_fixture(name, variant))
        .unwrap_or_else(|e| panic!("deserialize {name}/{variant} failed: {e}"))
}

/// Validates the SDK's *serialized output* against the `OpenAPI` schema, then runs
/// the strict-fidelity roundtrip on the raw fixture.
///
/// Validating `to_value(&deserialized)` (not the raw fixture) exercises the Rust
/// type against the published schema, so a bad `#[serde(rename)]` or a dropped
/// field is caught here rather than passing tautologically.
fn validate_and_roundtrip<T>(name: &str, variant: &str, schema: &str)
where
    T: Serialize + DeserializeOwned,
{
    let fixture = load_fixture(name, variant);
    let value: T = serde_json::from_value(fixture.clone())
        .unwrap_or_else(|e| panic!("deserialize {name}/{variant} failed: {e}"));
    let output = serde_json::to_value(&value)
        .unwrap_or_else(|e| panic!("serialize {name}/{variant} failed: {e}"));
    validate_openapi(&output, schema);
    roundtrip::<T>(fixture);
}

// ---------------------------------------------------------------------------
// Schema validation + strict roundtrip
// ---------------------------------------------------------------------------

#[test]
fn validation_error_min_validates_and_roundtrips() {
    validate_and_roundtrip::<ValidationError>("ValidationError", "min", "ValidationError");
}

#[test]
fn validation_error_max_validates_and_roundtrips() {
    validate_and_roundtrip::<ValidationError>("ValidationError", "max", "ValidationError");
}

#[test]
fn http_validation_error_min_validates_and_roundtrips() {
    validate_and_roundtrip::<HTTPValidationError>(
        "HTTPValidationError",
        "min",
        "HTTPValidationError",
    );
}

#[test]
fn http_validation_error_max_validates_and_roundtrips() {
    validate_and_roundtrip::<HTTPValidationError>(
        "HTTPValidationError",
        "max",
        "HTTPValidationError",
    );
}

// ---------------------------------------------------------------------------
// Optional-field presence / value asserts
// ---------------------------------------------------------------------------

#[test]
fn validation_error_optional_fields_absent_in_min() {
    let ve: ValidationError = deserialize_fixture("ValidationError", "min");
    assert!(ve.input.is_none());
    assert!(ve.ctx.is_none());
}

#[test]
fn validation_error_optional_fields_present_in_max() {
    let ve: ValidationError = deserialize_fixture("ValidationError", "max");

    // Assert the actual decoded values, not merely that they are `Some`.
    assert_eq!(
        ve.input.as_ref().expect("input present in max"),
        &json!("a_very_long_string_that_exceeds_the_maximum_allowed_length_for_this_field"),
    );
    let ctx = ve.ctx.as_ref().expect("ctx present in max");
    assert_eq!(ctx["max_length"], json!(255));
}

#[test]
fn http_validation_error_max_has_two_details() {
    let http: HTTPValidationError = deserialize_fixture("HTTPValidationError", "max");
    let errors = http.errors();
    assert_eq!(errors.len(), 2);

    // Verify content, not just count: the two details are the distinct ones
    // from the fixture, in order.
    assert_eq!(errors[0].error_type, "value_error.missing");
    assert_eq!(errors[0].msg, "field required");
    assert_eq!(errors[1].error_type, "value_error.any_str.max_length");
    assert_eq!(
        errors[1].ctx.as_ref().expect("ctx on second detail")["max_length"],
        json!(255),
    );
}

#[test]
fn loc_path_with_mixed_segments() {
    let ve: ValidationError = deserialize_fixture("ValidationError", "max");
    assert_eq!(ve.loc.len(), 4);
    assert_eq!(ve.loc[0], LocationSegment::String("body".to_string()));
    assert_eq!(ve.loc[1], LocationSegment::String("metadata".to_string()));
    assert_eq!(ve.loc[2], LocationSegment::Integer(0));
    assert_eq!(ve.loc[3], LocationSegment::String("key".to_string()));
}

// ---------------------------------------------------------------------------
// `error_type` -> `"type"` rename + `skip_serializing_if` (golden JSON)
// ---------------------------------------------------------------------------

#[test]
fn skip_serializing_none_optional_fields() {
    let ve: ValidationError = serde_json::from_value(json!({
        "loc": ["query"],
        "msg": "field required",
        "type": "value_error.missing"
    }))
    .unwrap();
    let json_val = serde_json::to_value(&ve).unwrap();
    let obj = json_val.as_object().unwrap();
    assert!(!obj.contains_key("input"));
    assert!(!obj.contains_key("ctx"));
}

#[test]
fn error_type_serializes_to_type_key_with_optionals_present() {
    // Positive counterpart to the absence test: when `input`/`ctx` are present
    // they must be emitted, and `error_type` must serialize under the `"type"`
    // key (the `#[serde(rename = "type")]` contract), never `"error_type"`.
    let ve: ValidationError = serde_json::from_value(json!({
        "loc": ["body", "name"],
        "msg": "invalid",
        "type": "value_error.custom",
        "input": 42,
        "ctx": { "max_length": 10 }
    }))
    .unwrap();
    assert_eq!(ve.error_type, "value_error.custom");

    let json_val = serde_json::to_value(&ve).unwrap();
    let obj = json_val.as_object().unwrap();
    assert_eq!(obj["type"], json!("value_error.custom"));
    assert!(!obj.contains_key("error_type"));
    assert_eq!(obj["input"], json!(42));
    assert_eq!(obj["ctx"], json!({ "max_length": 10 }));
}

// ---------------------------------------------------------------------------
// `LocationSegment` untagged-enum behaviour
// ---------------------------------------------------------------------------

#[test]
fn location_segment_string_and_integer() {
    let seg: LocationSegment = serde_json::from_value(json!("field_name")).unwrap();
    assert_eq!(seg, LocationSegment::String("field_name".to_string()));

    let seg: LocationSegment = serde_json::from_value(json!(3)).unwrap();
    assert_eq!(seg, LocationSegment::Integer(3));
}

#[test]
fn location_segment_integer_negative_and_i64_bounds() {
    // `Integer` is `i64`: negatives and the full i64 range must decode.
    let seg: LocationSegment = serde_json::from_value(json!(-7)).unwrap();
    assert_eq!(seg, LocationSegment::Integer(-7));

    let seg: LocationSegment = serde_json::from_value(json!(i64::MAX)).unwrap();
    assert_eq!(seg, LocationSegment::Integer(i64::MAX));

    let seg: LocationSegment = serde_json::from_value(json!(i64::MIN)).unwrap();
    assert_eq!(seg, LocationSegment::Integer(i64::MIN));

    // Above the i64 range `Integer` cannot hold the value, so it falls through
    // to the `Other` catch-all rather than failing deserialization.
    let seg: LocationSegment = serde_json::from_value(json!(u64::MAX)).unwrap();
    assert!(matches!(seg, LocationSegment::Other(_)));
}

#[test]
fn numeric_string_stays_string_not_integer() {
    // Untagged ordering gate: `String` is declared before `Integer`, so a JSON
    // *string* "3" must decode to `String("3")`, never coerce to `Integer(3)`.
    let seg: LocationSegment = serde_json::from_value(json!("3")).unwrap();
    assert_eq!(seg, LocationSegment::String("3".to_string()));
    assert!(!matches!(seg, LocationSegment::Integer(_)));
}

#[test]
fn location_segment_other_catches_non_string_non_integer() {
    // null / float loc elements must deserialize via the `Other` catch-all
    // rather than failing the whole error body.
    let seg: LocationSegment = serde_json::from_value(json!(null)).unwrap();
    assert!(matches!(seg, LocationSegment::Other(_)));

    let seg: LocationSegment = serde_json::from_value(json!(3.5)).unwrap();
    assert!(matches!(seg, LocationSegment::Other(_)));
}

#[test]
fn location_segment_display() {
    assert_eq!(
        LocationSegment::String("body".to_string()).to_string(),
        "body"
    );
    assert_eq!(LocationSegment::Integer(7).to_string(), "7");
}

// ---------------------------------------------------------------------------
// `loc` path edge cases
// ---------------------------------------------------------------------------

#[test]
fn loc_path_renders_dotted_indexed_path() {
    let ve: ValidationError = serde_json::from_value(json!({
        "loc": ["body", "items", 0, "name"],
        "msg": "field required",
        "type": "value_error.missing"
    }))
    .unwrap();
    assert_eq!(ve.loc_path(), "body.items[0].name");
}

#[test]
fn empty_loc_deserializes_and_roundtrips() {
    // A root-level error may carry an empty `loc`; it must decode, render to an
    // empty path, and survive validation/serialization unchanged.
    let ve: ValidationError = serde_json::from_value(json!({
        "loc": [],
        "msg": "root error",
        "type": "value_error"
    }))
    .unwrap();
    assert!(ve.loc.is_empty());
    assert_eq!(ve.loc_path(), "");

    let output = serde_json::to_value(&ve).unwrap();
    assert_eq!(output["loc"], json!([]));
    validate_openapi(&output, "ValidationError");
}

// ---------------------------------------------------------------------------
// Negative serde cases
// ---------------------------------------------------------------------------

#[test]
fn missing_required_fields_fail() {
    // Each of `loc` / `msg` / `type` is required; the error message must name
    // the missing field, so we assert the *cause*, not bare `is_err()`.
    let err = serde_json::from_value::<ValidationError>(json!({
        "msg": "m", "type": "t"
    }))
    .unwrap_err();
    assert!(
        err.to_string().contains("loc"),
        "expected missing `loc`: {err}"
    );

    let err = serde_json::from_value::<ValidationError>(json!({
        "loc": ["a"], "type": "t"
    }))
    .unwrap_err();
    assert!(
        err.to_string().contains("msg"),
        "expected missing `msg`: {err}"
    );

    let err = serde_json::from_value::<ValidationError>(json!({
        "loc": ["a"], "msg": "m"
    }))
    .unwrap_err();
    assert!(
        err.to_string().contains("type"),
        "expected missing `type`: {err}"
    );
}

#[test]
fn unknown_fields_are_tolerated() {
    // `ValidationError` has no `deny_unknown_fields`: an unexpected server field
    // (e.g. Pydantic v2's `url`) is ignored for forward-compatibility, and is
    // dropped on re-serialization.
    let ve: ValidationError = serde_json::from_value(json!({
        "loc": ["a"],
        "msg": "m",
        "type": "t",
        "url": "https://errors.pydantic.dev/2/v/missing"
    }))
    .unwrap();
    assert_eq!(ve.error_type, "t");

    let output = serde_json::to_value(&ve).unwrap();
    assert!(!output.as_object().unwrap().contains_key("url"));
}

// ---------------------------------------------------------------------------
// `HTTPValidationError.detail` untagged shapes
// ---------------------------------------------------------------------------

#[test]
fn http_validation_error_string_detail() {
    // FastAPI's `HTTPException(422, detail="msg")` returns a string, not an array.
    let http: HTTPValidationError = serde_json::from_value(json!({ "detail": "msg" })).unwrap();
    assert_eq!(http.detail, Detail::Message("msg".to_string()));
    assert_eq!(http.message(), Some("msg"));
    assert!(http.errors().is_empty());
}

#[test]
fn http_validation_error_absent_detail_defaults_empty() {
    // `detail` is not required per OpenAPI; an empty body must still deserialize.
    let http: HTTPValidationError = serde_json::from_value(json!({})).unwrap();
    assert!(http.errors().is_empty());
    assert_eq!(http.message(), None);
    assert_eq!(http.detail, Detail::Errors(Vec::new()));
}

#[test]
fn http_validation_error_null_detail_deserializes() {
    // An explicit `{"detail": null}` must not fail deserialization.
    let http: HTTPValidationError = serde_json::from_value(json!({ "detail": null })).unwrap();
    assert!(http.errors().is_empty());
    assert_eq!(http.message(), None);
    assert_eq!(http.detail, Detail::Null(()));
}

#[test]
fn http_validation_error_array_detail() {
    let http: HTTPValidationError = serde_json::from_value(json!({
        "detail": [
            {
                "loc": ["body", "name"],
                "msg": "field required",
                "type": "value_error.missing"
            }
        ]
    }))
    .unwrap();
    assert!(matches!(http.detail, Detail::Errors(_)));
    assert_eq!(http.errors().len(), 1);
    assert_eq!(http.message(), None);
}
