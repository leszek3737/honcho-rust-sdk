//! Round-trip and OpenAPI-validation tests for Peer types.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

mod common;
use common::*;

use honcho_ai::types::peer::{
    Peer, PeerCardConfiguration, PeerCardResponse, PeerCardSet, PeerConfig, PeerContext,
    PeerContextOptions, PeerCreate, PeerGet, PeerPage, PeerRepresentationGet, PeerUpdate,
};
use rstest::rstest;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// `OpenAPI` component name for a page of `Peer`s.
///
/// The generator emits `Page[Peer]` as `Page_Peer_` — note the **trailing
/// double underscore** (`[` and `]` each map to `_`). Binding it to a const
/// gives the awkward, typo-prone literal a single source of truth: it is written
/// (and reviewed) once here, not re-typed at each use site. The const is consumed
/// by `peer_page` below, so a typo in this value is *not* caught at compile time —
/// it surfaces at runtime as a "schema not found" failure.
const PAGE_PEER_SCHEMA: &str = "Page_Peer_";

/// Deserializes the fixture, validates the SDK's *serialized output* against the
/// `OpenAPI` schema, then asserts a strict round-trip.
///
/// Validating the SDK output (not the raw fixture) exercises the Rust type, so a
/// divergence between type and schema — a bad `#[serde(rename)]`, a dropped
/// field — is caught here. `roundtrip` then enforces byte-for-byte fidelity
/// (`canonicalize(output) == canonicalize(fixture)`).
fn do_test<T>(schema_name: &str, variant: &str)
where
    T: Serialize + DeserializeOwned,
{
    let fixture = load_fixture(schema_name, variant);
    let value: T = serde_json::from_value(fixture.clone()).unwrap();
    let output = serde_json::to_value(&value).unwrap();
    validate_openapi(&output, schema_name);
    roundtrip::<T>(fixture);
}

/// Generates an `rstest` covering both the `min` and `max` fixtures for a type
/// that has a backing `OpenAPI` schema. Collapses the per-schema `#[rstest]`
/// boilerplate (case list + `do_test` body) to a single line.
macro_rules! schema_roundtrip {
    ($name:ident, $ty:ty, $schema:expr) => {
        #[rstest]
        #[case::min("min")]
        #[case::max("max")]
        fn $name(#[case] variant: &str) {
            do_test::<$ty>($schema, variant);
        }
    };
}

// ---------------------------------------------------------------------------
// Per-schema round-trip + SDK-output validation
// ---------------------------------------------------------------------------

schema_roundtrip!(peer, Peer, "Peer");
schema_roundtrip!(peer_create, PeerCreate, "PeerCreate");
schema_roundtrip!(peer_update, PeerUpdate, "PeerUpdate");
schema_roundtrip!(peer_get, PeerGet, "PeerGet");
schema_roundtrip!(
    peer_card_configuration,
    PeerCardConfiguration,
    "PeerCardConfiguration"
);
schema_roundtrip!(peer_card_response, PeerCardResponse, "PeerCardResponse");
schema_roundtrip!(peer_card_set, PeerCardSet, "PeerCardSet");
schema_roundtrip!(peer_context, PeerContext, "PeerContext");
schema_roundtrip!(
    peer_representation_get,
    PeerRepresentationGet,
    "PeerRepresentationGet"
);
schema_roundtrip!(peer_page, PeerPage, PAGE_PEER_SCHEMA);

// ---------------------------------------------------------------------------
// PeerContextOptions — SDK-only request type, no OpenAPI schema
// ---------------------------------------------------------------------------

// `PeerContextOptions` is an SDK-side request-builder type with **no
// corresponding OpenAPI component schema**, so — unlike every type above — it is
// round-tripped *without* `validate_openapi`: there is nothing to validate it
// against. This asymmetry is intentional, not an omission.
#[rstest]
#[case::min("min")]
#[case::max("max")]
fn peer_context_options_roundtrip(#[case] variant: &str) {
    let fixture = load_fixture("PeerContextOptions", variant);
    roundtrip::<PeerContextOptions>(fixture);
}

#[test]
fn peer_context_options_max_values() {
    // A stable round-trip proves the encoding is a fixed point, not that each
    // key lands on the right field — pin the full populated mapping explicitly.
    let fixture = load_fixture("PeerContextOptions", "max");
    let opts: PeerContextOptions = serde_json::from_value(fixture).unwrap();
    assert_eq!(opts.target.as_deref(), Some("peer_alpha"));
    assert_eq!(
        opts.search_query.as_deref(),
        Some("recent project decisions")
    );
    assert_eq!(opts.search_top_k, Some(15));
    assert_eq!(opts.search_max_distance, Some(0.6));
    assert_eq!(opts.include_most_frequent, Some(true));
    assert_eq!(opts.max_conclusions, Some(25));
}

// ---------------------------------------------------------------------------
// PeerConfig — typed peer configuration
// ---------------------------------------------------------------------------

// Like `PeerContextOptions` above, `PeerConfig` is **absent from the OpenAPI
// spec** (no corresponding component schema), so its tests round-trip and pin
// values *without* `validate_openapi` — there is nothing to validate against.
// The omission is intentional, mirroring the request-type asymmetry noted above.
#[test]
fn peer_config_defaults() {
    let cfg: PeerConfig = serde_json::from_value(serde_json::json!({})).unwrap();
    assert!(cfg.observe_me.is_none());
    assert!(cfg.observe_others.is_none());
    // Absent optionals must serialize away entirely (`skip_serializing_if`), so
    // an empty config is the empty object — never `{"observe_me": null, ..}`.
    assert_eq!(serde_json::to_value(&cfg).unwrap(), serde_json::json!({}));
}

#[test]
fn peer_config_roundtrip() {
    let fixture = serde_json::json!({"observe_me": true, "observe_others": false});
    let cfg: PeerConfig = serde_json::from_value(fixture.clone()).unwrap();
    // Value coverage: round-trip stability alone would not catch the two `bool`
    // fields being swapped, so pin the mapping.
    assert_eq!(cfg.observe_me, Some(true));
    assert_eq!(cfg.observe_others, Some(false));
    roundtrip::<PeerConfig>(fixture);
}

// ---------------------------------------------------------------------------
// Negative serde cases
// ---------------------------------------------------------------------------

#[test]
fn peer_config_rejects_wrong_type() {
    // `observe_me` is `Option<bool>`; a string value must fail to deserialize.
    // Assert the *cause* (a type mismatch), not a bare `is_err()`.
    let bad = serde_json::json!({"observe_me": "yes"});
    let err = serde_json::from_value::<PeerConfig>(bad).unwrap_err();
    assert!(
        err.to_string().contains("invalid type"),
        "expected an invalid-type error: {err}"
    );
}

#[test]
fn peer_config_rejects_malformed_json() {
    // Truncated JSON text must fail at the parser (syntax/EOF), not silently
    // default — assert the *cause*, not a bare `is_err()`.
    let err = serde_json::from_str::<PeerConfig>("{\"observe_me\":").unwrap_err();
    assert!(
        err.is_syntax() || err.is_eof(),
        "expected a syntax/EOF parse error: {err}"
    );
}

#[test]
fn peer_rejects_missing_required_field() {
    // `Peer` requires `workspace_id` and `created_at`; omitting them is an error.
    // Pin the *cause* to a concrete field — `workspace_id`, the first required
    // field missing in declaration order — so a regression that drops a different
    // field (or fails for an unrelated reason) cannot pass this test silently.
    let bad = serde_json::json!({"id": "p1"});
    let err = serde_json::from_value::<Peer>(bad).unwrap_err();
    assert!(
        err.to_string().contains("missing field `workspace_id`"),
        "expected missing `workspace_id`: {err}"
    );
}

#[test]
fn peer_config_tolerates_unknown_field() {
    // GATE / INFO: `PeerConfig` has no `#[serde(deny_unknown_fields)]`, so an
    // unknown key is silently accepted and dropped on re-serialize. This asserts
    // *current* behavior; if a future PR adds `deny_unknown_fields`, flip this to
    // `is_err()`. (The strict `roundtrip` above does not catch this because the
    // dropped key never originates from a known field.)
    let extra = serde_json::json!({"observe_me": true, "unknown_field": 1});
    let cfg: PeerConfig = serde_json::from_value(extra).unwrap();
    assert_eq!(cfg.observe_me, Some(true));
    assert_eq!(
        serde_json::to_value(&cfg).unwrap(),
        serde_json::json!({"observe_me": true})
    );
}
