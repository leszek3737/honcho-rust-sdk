#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

mod common;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::json;

use honcho_ai::types::common::{DreamConfiguration, ReasoningConfiguration};
use honcho_ai::types::dialectic::{DialecticOptions, ReasoningLevel, RepresentationResponse};
use honcho_ai::types::dream::{DreamType, QueueStatus, ScheduleDreamRequest};

use common::{load_fixture, roundtrip, validate_openapi};

/// Mirrors the private `MAX_DIALECTIC_QUERY_CHARS` in `src/types/dialectic.rs`.
///
/// The source constant is module-private (not re-exported), so it cannot be
/// imported here. Keeping a single local mirror drives both the generated input
/// length and the expected error message, instead of scattering `10_000` /
/// `10_001` literals across the length-validation tests.
///
/// Deferred src dep: expose `MAX_DIALECTIC_QUERY_CHARS` (`pub`) from src so this
/// mirror can be replaced by a direct import and can never silently desync.
const MAX_DIALECTIC_QUERY_CHARS: usize = 10_000;

// ── Generic fixture case ────────────────────────────────────────────
//
// One `#[test]` per (schema, variant): load the fixture exactly once, then
// assert BOTH directions of the contract:
//   1. the SDK's *serialized output* (not the raw fixture) conforms to the
//      published OpenAPI schema — catches SDK↔schema parity drift;
//   2. the strict round-trip (A0 keystone) — `canonicalize(fixture) ==
//      canonicalize(SDK output)` — catches any silently dropped/renamed field.

/// Loads `<name>/<variant>.json` once and runs SDK-output `OpenAPI` validation
/// plus a strict round-trip for `T`.
fn fixture_case<T>(name: &str, variant: &str)
where
    T: Serialize + DeserializeOwned,
{
    let fixture = load_fixture(name, variant);

    // Validate the SDK's own serialized output against the schema, not the
    // raw fixture, so a bad `#[serde(rename)]`/missing field is caught.
    let parsed: T = serde_json::from_value(fixture.clone())
        .unwrap_or_else(|e| panic!("deserialize {name}/{variant}: {e}"));
    let output =
        serde_json::to_value(&parsed).unwrap_or_else(|e| panic!("serialize {name}/{variant}: {e}"));
    validate_openapi(&output, name);

    // Strict fidelity + self-idempotence (A0 keystone).
    roundtrip::<T>(fixture);
}

/// Generates `min`/`max` fixture-case tests for `$ty` under module `$module`.
///
/// Collapses the former ~22-fn validate/round-trip boilerplate; the schema name
/// (used for both the fixture dir and `OpenAPI` lookup) is `$schema`.
macro_rules! fixture_tests {
    ($module:ident, $ty:ty, $schema:literal) => {
        mod $module {
            use super::*;

            #[test]
            fn min() {
                fixture_case::<$ty>($schema, "min");
            }

            #[test]
            fn max() {
                fixture_case::<$ty>($schema, "max");
            }
        }
    };
}

// ── Dialectic ───────────────────────────────────────────────────────

fixture_tests!(dialectic_options, DialecticOptions, "DialecticOptions");
fixture_tests!(
    representation_response,
    RepresentationResponse,
    "RepresentationResponse"
);

// ── Dream ───────────────────────────────────────────────────────────

fixture_tests!(
    dream_configuration,
    DreamConfiguration,
    "DreamConfiguration"
);
fixture_tests!(
    reasoning_configuration,
    ReasoningConfiguration,
    "ReasoningConfiguration"
);
fixture_tests!(
    schedule_dream_request,
    ScheduleDreamRequest,
    "ScheduleDreamRequest"
);
fixture_tests!(queue_status, QueueStatus, "QueueStatus");

// ── DialecticOptions: golden-JSON `skip_serializing_if` ─────────────

#[test]
fn dialectic_options_omits_default_stream_and_reasoning_level() {
    // `stream=false` and `reasoning_level=Low` are the derived defaults and
    // carry `skip_serializing_if`, so they must vanish from the wire form —
    // only `query` survives. A lost `skip_serializing_if` fails this exact assert.
    let options = DialecticOptions::builder().query("hi").build();
    let value = serde_json::to_value(&options).unwrap();
    assert_eq!(value, json!({ "query": "hi" }));
}

#[test]
fn dialectic_options_emits_non_default_stream_and_reasoning_level() {
    // The positive side: once set away from their defaults, both fields appear.
    let options = DialecticOptions::builder()
        .query("hi")
        .stream(true)
        .reasoning_level(ReasoningLevel::High)
        .build();
    let value = serde_json::to_value(&options).unwrap();
    assert_eq!(
        value,
        json!({ "query": "hi", "stream": true, "reasoning_level": "high" })
    );
}

// ── DialecticOptions: query validation ──────────────────────────────

#[test]
fn dialectic_options_validate_rejects_empty_query() {
    // `validate_dialectic_query` rejects the empty (and whitespace-only) string.
    let options = DialecticOptions::builder().query("").build();
    let err = options.validate().unwrap_err();
    assert_eq!(err.code(), "validation_error");
    assert_eq!(err.message(), "query must not be empty");
}

#[test]
fn dialectic_options_validate_accepts_max_chars() {
    let options = DialecticOptions::builder()
        .query("a".repeat(MAX_DIALECTIC_QUERY_CHARS))
        .build();
    options.validate().unwrap();
}

#[test]
fn dialectic_options_validate_rejects_over_max_chars() {
    let options = DialecticOptions::builder()
        .query("a".repeat(MAX_DIALECTIC_QUERY_CHARS + 1))
        .build();
    let err = options.validate().unwrap_err();
    assert_eq!(err.code(), "validation_error");
    // Drive the expected message from the same constant as the input length,
    // not a hard-coded `"... 10000 ..."` literal.
    let expected = format!("query must be at most {MAX_DIALECTIC_QUERY_CHARS} characters");
    assert_eq!(err.message(), expected.as_str());
}

#[test]
fn dialectic_options_validate_counts_unicode_chars_not_bytes() {
    // A 4-byte char counts as one character: MAX crabs must still pass.
    let options = DialecticOptions::builder()
        .query("🦀".repeat(MAX_DIALECTIC_QUERY_CHARS))
        .build();
    options.validate().unwrap();
}

#[test]
fn dialectic_options_rejects_missing_query() {
    // `query` is required: an object without it fails to deserialize.
    let err = serde_json::from_value::<DialecticOptions>(json!({})).unwrap_err();
    assert!(
        err.to_string().contains("missing field"),
        "expected a missing-field error, got: {err}"
    );
}

// ── ReasoningLevel: variant coverage & serde ────────────────────────

/// Every *known* `ReasoningLevel` variant. Hand-maintained because the enum is
/// `#[non_exhaustive]` and the crate does not derive an iterator (e.g.
/// `strum::EnumIter`), so the variant set is not machine-enumerable from this
/// downstream test crate.
///
/// Deferred src dep: derive `strum::EnumIter` (or expose a `const ALL: &[Self]`)
/// on `ReasoningLevel` so adding a variant mechanically forces an update here.
/// Until then this list and [`reasoning_wire`] are the manual single source of
/// truth (the `_` arm in `reasoning_wire` documents the gap).
const KNOWN_REASONING_LEVELS: [ReasoningLevel; 6] = [
    ReasoningLevel::Minimal,
    ReasoningLevel::Low,
    ReasoningLevel::Medium,
    ReasoningLevel::High,
    ReasoningLevel::Max,
    ReasoningLevel::Unknown,
];

/// Wire encoding of a known `ReasoningLevel`.
///
/// The `_` arm is mandatory because `ReasoningLevel` is `#[non_exhaustive]`
/// (a downstream `match` cannot be exhaustive). It panics so an untracked
/// future variant fails loudly here rather than passing silently.
fn reasoning_wire(level: ReasoningLevel) -> &'static str {
    match level {
        ReasoningLevel::Minimal => "minimal",
        ReasoningLevel::Low => "low",
        ReasoningLevel::Medium => "medium",
        ReasoningLevel::High => "high",
        ReasoningLevel::Max => "max",
        ReasoningLevel::Unknown => "unknown",
        _ => panic!(
            "unhandled ReasoningLevel variant — update reasoning_wire + KNOWN_REASONING_LEVELS"
        ),
    }
}

#[test]
fn reasoning_level_wire_encoding_is_bijective() {
    for level in KNOWN_REASONING_LEVELS {
        let wire = reasoning_wire(level);
        assert_eq!(
            serde_json::to_value(level).unwrap(),
            json!(wire),
            "serialize mismatch for {level:?}"
        );
        let parsed: ReasoningLevel = serde_json::from_value(json!(wire)).unwrap();
        assert_eq!(parsed, level, "deserialize mismatch for {wire}");
    }
}

#[test]
fn reasoning_level_default_is_low() {
    assert_eq!(ReasoningLevel::default(), ReasoningLevel::Low);
}

#[test]
fn reasoning_level_unknown_string_maps_to_unknown() {
    // src models forward-compat via `#[serde(other)]`: an unrecognized wire
    // string is NOT a hard error — it deserializes to `Unknown`. This is the
    // intended contract, so the classic "unknown variant => Err" does not apply.
    let parsed: ReasoningLevel = serde_json::from_value(json!("totally_new_level")).unwrap();
    assert_eq!(parsed, ReasoningLevel::Unknown);
}

#[test]
fn reasoning_level_rejects_non_string() {
    // A non-string is a wrong-type error (the `#[serde(other)]` catch-all only
    // covers unknown *strings*, not foreign JSON types).
    let err = serde_json::from_value::<ReasoningLevel>(json!(123)).unwrap_err();
    assert!(
        err.to_string().contains("invalid type"),
        "expected an invalid-type error, got: {err}"
    );
}

// ── DreamType: variant coverage & serde ─────────────────────────────

/// Every *known* `DreamType` variant — see [`KNOWN_REASONING_LEVELS`] for why
/// this is hand-maintained (`#[non_exhaustive]`, no derived iterator).
const KNOWN_DREAM_TYPES: [DreamType; 2] = [DreamType::Omni, DreamType::Unknown];

/// Wire encoding of a known `DreamType`. The `_` arm is mandatory because the
/// enum is `#[non_exhaustive]`; it panics on an untracked future variant.
fn dream_wire(dream_type: DreamType) -> &'static str {
    match dream_type {
        DreamType::Omni => "omni",
        DreamType::Unknown => "unknown",
        _ => panic!("unhandled DreamType variant — update dream_wire + KNOWN_DREAM_TYPES"),
    }
}

#[test]
fn dream_type_wire_encoding_is_bijective() {
    for dream_type in KNOWN_DREAM_TYPES {
        let wire = dream_wire(dream_type);
        assert_eq!(
            serde_json::to_value(dream_type).unwrap(),
            json!(wire),
            "serialize mismatch for {dream_type:?}"
        );
        let parsed: DreamType = serde_json::from_value(json!(wire)).unwrap();
        assert_eq!(parsed, dream_type, "deserialize mismatch for {wire}");
    }
}

#[test]
fn dream_type_unknown_string_maps_to_unknown() {
    // Forward-compat catch-all (`#[serde(other)]`): unknown wire string =>
    // `Unknown`, not an error.
    let parsed: DreamType = serde_json::from_value(json!("totally_new_dream")).unwrap();
    assert_eq!(parsed, DreamType::Unknown);
}

#[test]
fn dream_type_rejects_non_string() {
    let err = serde_json::from_value::<DreamType>(json!(true)).unwrap_err();
    assert!(
        err.to_string().contains("invalid type"),
        "expected an invalid-type error, got: {err}"
    );
}
