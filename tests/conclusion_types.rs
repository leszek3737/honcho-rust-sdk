#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::float_cmp,
    missing_docs
)]

mod common;

use common::{load_fixture, roundtrip, validate_openapi};

use honcho_ai::types::conclusion::{
    ConclusionBatchCreate, ConclusionCreate, ConclusionFilters, ConclusionGet, ConclusionPage,
    ConclusionQuery, ConclusionResponse,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::json;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Deserializes a fixture into `T`, re-serializes the SDK value, and validates
/// that **SDK output** (not the raw fixture) against the named `OpenAPI`
/// schema.
///
/// Validating the round-tripped output is what makes the schema check
/// meaningful: a type that drops or renames a field diverges from the schema
/// here, whereas validating the raw fixture would never exercise the SDK type.
fn validate_sdk_output<T>(fixture: &str, variant: &str, schema: &str)
where
    T: Serialize + DeserializeOwned,
{
    let value: T = serde_json::from_value(load_fixture(fixture, variant)).unwrap();
    let output = serde_json::to_value(&value).unwrap();
    validate_openapi(&output, schema);
}

/// Generates the four standard fixture tests for a conclusion type:
/// `<prefix>_{min,max}_validates` (SDK-output schema validation) and
/// `<prefix>_roundtrip_{min,max}` (strict fixture-equality fidelity, inherited
/// from the `common::roundtrip` keystone).
macro_rules! case {
    ($prefix:ident, $ty:ty, $fixture:literal, $schema:literal) => {
        paste::paste! {
            #[test]
            fn [<$prefix _min_validates>]() {
                validate_sdk_output::<$ty>($fixture, "min", $schema);
            }

            #[test]
            fn [<$prefix _max_validates>]() {
                validate_sdk_output::<$ty>($fixture, "max", $schema);
            }

            #[test]
            fn [<$prefix _roundtrip_min>]() {
                roundtrip::<$ty>(load_fixture($fixture, "min"));
            }

            #[test]
            fn [<$prefix _roundtrip_max>]() {
                roundtrip::<$ty>(load_fixture($fixture, "max"));
            }
        }
    };
}

// ---------------------------------------------------------------------------
// Standard fixture cases (SDK-output validation + strict round-trip)
// ---------------------------------------------------------------------------

case!(conclusion, ConclusionResponse, "Conclusion", "Conclusion");
case!(
    conclusion_create,
    ConclusionCreate,
    "ConclusionCreate",
    "ConclusionCreate"
);
case!(
    conclusion_batch_create,
    ConclusionBatchCreate,
    "ConclusionBatchCreate",
    "ConclusionBatchCreate"
);
case!(
    conclusion_get,
    ConclusionGet,
    "ConclusionGet",
    "ConclusionGet"
);
case!(
    page_conclusion,
    ConclusionPage,
    "Page_Conclusion_",
    "Page_Conclusion_"
);

// `ConclusionFilters` has no OpenAPI schema (the spec models conclusion filters
// as a free-form `additionalProperties: true` object), so it gets strict
// round-trip coverage only — no `validate_openapi` call.
#[test]
fn conclusion_filters_roundtrip_min() {
    roundtrip::<ConclusionFilters>(load_fixture("ConclusionFilters", "min"));
}

#[test]
fn conclusion_filters_roundtrip_max() {
    roundtrip::<ConclusionFilters>(load_fixture("ConclusionFilters", "max"));
}

// ---------------------------------------------------------------------------
// ConclusionQuery — handled outside `case!`
//
// `top_k` is always serialized: it carries serde `default` with no
// `skip_serializing_if`, so the SDK injects `top_k: 10` for the `min` fixture,
// which deliberately omits it. A strict round-trip on `min` would therefore
// fail on an intentional, documented addition (see `ConclusionQuery::top_k`'s
// "always send top_k" contract), so `min` gets a dedicated golden-output test
// instead. `max` already carries `top_k`, so it round-trips strictly.
// ---------------------------------------------------------------------------

#[test]
fn conclusion_query_min_validates() {
    validate_sdk_output::<ConclusionQuery>("ConclusionQuery", "min", "ConclusionQuery");
}

#[test]
fn conclusion_query_max_validates() {
    validate_sdk_output::<ConclusionQuery>("ConclusionQuery", "max", "ConclusionQuery");
}

#[test]
fn conclusion_query_roundtrip_max() {
    roundtrip::<ConclusionQuery>(load_fixture("ConclusionQuery", "max"));
}

#[test]
fn conclusion_query_min_injects_default_top_k() {
    // The `min` fixture omits `top_k`; the SDK fills the documented default and
    // always emits it, so the wire output carries exactly one extra key.
    let q: ConclusionQuery =
        serde_json::from_value(load_fixture("ConclusionQuery", "min")).unwrap();
    assert_eq!(q.top_k, 10, "absent top_k must deserialize to the default");
    assert!(q.distance.is_none());
    assert!(q.filters.is_none());

    let output = serde_json::to_value(&q).unwrap();
    assert_eq!(
        output,
        json!({ "query": "user preferences", "top_k": 10 }),
        "min query must serialize to query + injected default top_k only"
    );
}

#[test]
fn conclusion_query_default_top_k_is_indistinguishable_on_wire() {
    // `top_k` has no skip predicate, so an *injected* default `10` and an
    // *explicit* `10` serialize identically — the documented "always send
    // top_k" contract, which removes the absent-vs-default ambiguity.
    let injected: ConclusionQuery = serde_json::from_value(json!({ "query": "q" })).unwrap();
    let explicit: ConclusionQuery =
        serde_json::from_value(json!({ "query": "q", "top_k": 10 })).unwrap();
    assert_eq!(
        serde_json::to_value(&injected).unwrap(),
        serde_json::to_value(&explicit).unwrap()
    );
}

// ---------------------------------------------------------------------------
// Value-mapping asserts
//
// Round-trip stability proves the encoding is a fixed point; it does NOT prove
// each JSON key maps to the intended field. These assert the actual mapping.
// ---------------------------------------------------------------------------

#[test]
fn conclusion_max_field_mapping() {
    let c: ConclusionResponse = serde_json::from_value(load_fixture("Conclusion", "max")).unwrap();
    assert_eq!(c.id, "concl_xyz");
    assert_eq!(
        c.content,
        "User tends to ask follow-up questions when unsure about a topic, \
         indicating a methodical learning style"
    );
    assert_eq!(c.observer_id, "agent_observer");
    assert_eq!(c.observed_id, "user_42");
    assert_eq!(c.session_id.as_deref(), Some("sess_abc"));
    // Assert the datetime via its serialized form to avoid pulling chrono in.
    let v = serde_json::to_value(&c).unwrap();
    assert_eq!(v["created_at"], json!("2025-06-15T12:30:45Z"));
}

#[test]
fn conclusion_create_max_field_mapping() {
    let c: ConclusionCreate =
        serde_json::from_value(load_fixture("ConclusionCreate", "max")).unwrap();
    assert_eq!(c.observer_id, "agent_1");
    assert_eq!(c.observed_id, "user_99");
    assert_eq!(c.session_id.as_deref(), Some("sess_xyz"));
    assert!(
        c.content
            .starts_with("User demonstrates strong analytical thinking")
    );
}

#[test]
fn conclusion_filters_max_field_mapping() {
    let f: ConclusionFilters =
        serde_json::from_value(load_fixture("ConclusionFilters", "max")).unwrap();
    assert_eq!(f.observer_id.as_deref(), Some("peer_obs_1"));
    assert_eq!(f.observed_id.as_deref(), Some("peer_obs_2"));
    assert_eq!(f.session_id.as_deref(), Some("sess_abc"));
}

#[test]
fn conclusion_get_max_unwraps_filters() {
    let g: ConclusionGet = serde_json::from_value(load_fixture("ConclusionGet", "max")).unwrap();
    let f = g.filters.expect("max fixture carries filters");
    assert_eq!(f.session_id.as_deref(), Some("sess_abc"));
    assert_eq!(f.observer_id.as_deref(), Some("agent_1"));
    assert!(f.observed_id.is_none());
}

#[test]
fn conclusion_get_min_has_no_filters() {
    let g: ConclusionGet = serde_json::from_value(load_fixture("ConclusionGet", "min")).unwrap();
    assert!(g.filters.is_none());
    // `filters` is `skip_serializing_if = None` → absent, never `null`.
    let v = serde_json::to_value(&g).unwrap();
    assert!(
        v.get("filters").is_none(),
        "empty ConclusionGet must serialize to {{}}"
    );
}

#[test]
fn conclusion_query_max_field_mapping() {
    let q: ConclusionQuery =
        serde_json::from_value(load_fixture("ConclusionQuery", "max")).unwrap();
    assert_eq!(q.query, "programming language preferences and experience");
    assert_eq!(q.top_k, 5);
    assert_eq!(q.distance, Some(0.75));
    let f = q.filters.expect("max fixture carries filters");
    assert_eq!(f.session_id.as_deref(), Some("sess_abc"));
    assert!(f.observer_id.is_none());
}

#[test]
fn page_conclusion_max_field_mapping() {
    let p: ConclusionPage =
        serde_json::from_value(load_fixture("Page_Conclusion_", "max")).unwrap();
    assert_eq!(p.page(), 1);
    assert_eq!(p.size(), 10);
    assert_eq!(p.total(), 2);
    assert_eq!(p.pages(), 1);
    let items = p.items_ref();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].id, "concl_a");
    assert!(items[0].session_id.is_none());
    assert_eq!(items[1].id, "concl_b");
    assert_eq!(items[1].session_id.as_deref(), Some("sess_xyz"));
}

#[test]
fn conclusion_batch_create_max_field_mapping() {
    let b: ConclusionBatchCreate =
        serde_json::from_value(load_fixture("ConclusionBatchCreate", "max")).unwrap();
    assert_eq!(b.conclusions.len(), 2);
    assert_eq!(b.conclusions[0].observer_id, "agent_1");
    assert_eq!(b.conclusions[1].session_id.as_deref(), Some("sess_a"));
}

// ---------------------------------------------------------------------------
// Negative serde cases — assert the *cause*, not bare stability
// ---------------------------------------------------------------------------

#[test]
fn conclusion_create_missing_content_is_err() {
    let e = serde_json::from_value::<ConclusionCreate>(
        json!({ "observer_id": "a", "observed_id": "b" }),
    )
    .unwrap_err();
    assert!(
        e.to_string().contains("missing field `content`"),
        "missing required `content` must surface as a missing-field error, got: {e}"
    );
}

#[test]
fn conclusion_create_wrong_type_content_is_err() {
    let e = serde_json::from_value::<ConclusionCreate>(json!({
        "content": 123, "observer_id": "a", "observed_id": "b"
    }))
    .unwrap_err();
    assert!(
        e.to_string().contains("invalid type"),
        "numeric `content` must surface as an invalid-type error (expected string), got: {e}"
    );
}

#[test]
fn conclusion_response_missing_created_at_is_err() {
    let e = serde_json::from_value::<ConclusionResponse>(json!({
        "id": "i", "content": "c", "observer_id": "o", "observed_id": "d"
    }))
    .unwrap_err();
    assert!(
        e.to_string().contains("missing field `created_at`"),
        "missing required `created_at` must surface as a missing-field error, got: {e}"
    );
}

#[test]
fn conclusion_query_missing_query_is_err() {
    let e = serde_json::from_value::<ConclusionQuery>(json!({ "top_k": 5 })).unwrap_err();
    assert!(
        e.to_string().contains("missing field `query`"),
        "missing required `query` must surface as a missing-field error, got: {e}"
    );
}

#[test]
fn conclusion_query_wrong_type_top_k_is_err() {
    let e = serde_json::from_value::<ConclusionQuery>(json!({ "query": "q", "top_k": "five" }))
        .unwrap_err();
    assert!(
        e.to_string().contains("invalid type"),
        "string `top_k` must surface as an invalid-type error (expected integer), got: {e}"
    );
}

#[test]
fn conclusion_create_null_session_id_is_none_and_omitted() {
    // `null` and absence both map to `None` for an `Option` field...
    let c: ConclusionCreate = serde_json::from_value(json!({
        "content": "x", "observer_id": "a", "observed_id": "b", "session_id": null
    }))
    .unwrap();
    assert!(
        c.session_id.is_none(),
        "explicit null session_id maps to None"
    );

    // ...and `skip_serializing_if` means `None` is omitted, never re-emitted as
    // `null`, so the round-trip is asymmetric for explicit-null input.
    let v = serde_json::to_value(&c).unwrap();
    assert!(
        v.get("session_id").is_none(),
        "None session_id must be omitted, not serialized as null"
    );
}

// ---------------------------------------------------------------------------
// Constraint GATES — schema declares bounds the SDK type does not enforce
//
// The OpenAPI schema constrains content (1..=65535), batch size (1..=100),
// top_k (1..=100), and distance (0.0..=1.0), but the SDK models them as raw
// String / Vec / u32 / f64. These tests pin TODAY's lenient behavior; if
// PR5/PR6 introduce validated newtypes, each should flip to asserting `Err`.
// ---------------------------------------------------------------------------

#[test]
fn conclusion_create_empty_content_currently_accepted() {
    // GATE: schema requires content length 1..=65535; the type does not check.
    let c: ConclusionCreate = serde_json::from_value(json!({
        "content": "", "observer_id": "a", "observed_id": "b"
    }))
    .unwrap();
    assert_eq!(c.content, "");
}

#[test]
fn conclusion_batch_create_empty_array_currently_accepted() {
    // GATE: schema requires 1..=100 items; the type is a raw Vec with no bound.
    let b: ConclusionBatchCreate = serde_json::from_value(json!({ "conclusions": [] })).unwrap();
    assert!(b.conclusions.is_empty());
}

#[test]
fn conclusion_query_top_k_out_of_range_currently_accepted() {
    // GATE: schema constrains top_k to 1..=100; the type is a raw u32.
    for k in [0_u32, 500_u32] {
        let q: ConclusionQuery =
            serde_json::from_value(json!({ "query": "q", "top_k": k })).unwrap();
        assert_eq!(q.top_k, k);
    }
}

#[test]
fn conclusion_query_distance_out_of_range_currently_accepted() {
    // GATE: schema constrains distance to 0.0..=1.0; the type is a raw f64.
    for d in [2.0_f64, -1.0_f64] {
        let q: ConclusionQuery =
            serde_json::from_value(json!({ "query": "q", "distance": d })).unwrap();
        assert_eq!(q.distance, Some(d));
    }
}

#[test]
fn conclusion_create_ignores_unknown_field_currently() {
    // GATE: no `#[serde(deny_unknown_fields)]`, so unknown keys are silently
    // dropped on input. If PR5/PR6 add the attribute, this becomes `Err`.
    let c: ConclusionCreate = serde_json::from_value(json!({
        "content": "x", "observer_id": "a", "observed_id": "b", "bogus": true
    }))
    .unwrap();
    assert_eq!(c.content, "x");
}
