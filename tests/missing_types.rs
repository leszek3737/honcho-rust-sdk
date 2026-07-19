#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

mod common;
use common::{load_fixture, roundtrip};

use honcho_ai::types::peer::PeerContextOptions;
use honcho_ai::types::session::SessionListOptions;
use serde_json::json;

// --- SessionListOptions (has Deserialize + builder) ---

#[test]
fn session_list_options_min_materializes_defaults() {
    // The `min` fixture is `{}`, but `SessionListOptions` marks
    // page/size/reverse with `#[serde(default)]` and NO `skip_serializing_if`,
    // so deserializing `{}` and re-serializing *materializes* those defaults
    // (the same deliberate "always send the value, never make the server infer
    // intent from omission" stance as `ConclusionQuery::top_k`). Under the A0
    // strict helper the bare `{}` fixture is therefore not a fixed point, so we
    // assert the real canonical shape directly instead of `roundtrip(min)`.
    // Deserialize-side `{}` tolerance is covered by
    // `session_list_options_serde_defaults`.
    let opts: SessionListOptions =
        serde_json::from_value(load_fixture("SessionListOptions", "min")).unwrap();
    let json = serde_json::to_value(&opts).unwrap();
    assert_eq!(json, json!({ "page": 1, "size": 50, "reverse": false }));
}

#[test]
fn session_list_options_roundtrip_max() {
    // Cleanest demonstrator of the A0 strict fix: the `max` fixture carries
    // `filters`. With the old lossy helper a dropped `filters` still passed
    // because `None` -> omit -> `None` was symmetric on both sides. `filters`
    // is modeled in `src/types/session.rs`, so strict fidelity now both passes
    // and pins the behavior.
    roundtrip::<SessionListOptions>(load_fixture("SessionListOptions", "max"));
}

#[test]
fn session_list_options_serde_defaults() {
    // serde path: an empty object fills page/size/reverse via the
    // `#[serde(default = ..)]` fns. Distinct from the builder path below.
    let opts: SessionListOptions = serde_json::from_value(json!({})).unwrap();
    assert_eq!(opts.page, 1);
    assert_eq!(opts.size, 50);
    assert!(!opts.reverse);
    assert!(opts.filters.is_none());
}

#[test]
fn session_list_options_builder_defaults() {
    // Builder path: `#[builder(default = default_page()/default_size())]` is a
    // *separate* code path from the serde defaults above and was uncovered. An
    // empty builder must produce the same values.
    let opts = SessionListOptions::builder().build();
    assert_eq!(opts.page, 1);
    assert_eq!(opts.size, 50);
    assert!(!opts.reverse);
    assert!(opts.filters.is_none());
}

#[test]
fn session_list_options_builder() {
    let opts = SessionListOptions::builder()
        .page(2)
        .size(25)
        .reverse(true)
        .build();
    let json = serde_json::to_value(&opts).unwrap();
    assert_eq!(json["page"], 2);
    assert_eq!(json["size"], 25);
    assert_eq!(json["reverse"], true);
    // Negative-shape: `filters` was never set, so `skip_serializing_if` must
    // omit it entirely (it must not leak as an explicit `null`).
    assert!(json.get("filters").is_none());
}

#[test]
fn session_list_options_size_is_unvalidated() {
    // GATE / m05 gap: `size` is a raw `u64` documented as "Must be in `1..=100`"
    // but the type enforces nothing — `0` and `500` both deserialize without
    // error. This locks the *current* lenient behavior (matching the deliberate
    // client-side "accept, let the server reject" stance used elsewhere, e.g.
    // `PeerContextOptions::search_max_distance`).
    //
    // Deferred src change (`src/types/session.rs`): a validated `PageSize`
    // newtype or a `SessionListOptions::validate()`. When it lands, flip these
    // to `unwrap_err()` / `is_err()`.
    let zero: SessionListOptions = serde_json::from_value(json!({ "size": 0 })).unwrap();
    assert_eq!(zero.size, 0);
    let over: SessionListOptions = serde_json::from_value(json!({ "size": 500 })).unwrap();
    assert_eq!(over.size, 500);
}

// --- PeerContextOptions (has Deserialize + builder, fixtures exist) ---

#[test]
fn peer_context_options_roundtrip_min() {
    roundtrip::<PeerContextOptions>(load_fixture("PeerContextOptions", "min"));
}

#[test]
fn peer_context_options_roundtrip_max() {
    roundtrip::<PeerContextOptions>(load_fixture("PeerContextOptions", "max"));
}

#[test]
fn peer_context_options_empty() {
    let opts: PeerContextOptions = serde_json::from_value(json!({})).unwrap();
    let val = serde_json::to_value(&opts).unwrap();
    assert_eq!(val, json!({}));
}

#[test]
fn peer_context_options_builder() {
    // Builder coverage parity with the other two option types (was missing —
    // an asymmetry the other tests already exercised).
    let opts = PeerContextOptions::builder()
        .target("peer_alpha")
        .search_query("recent decisions")
        .search_top_k(15)
        .build();
    let json = serde_json::to_value(&opts).unwrap();
    assert_eq!(json["target"], "peer_alpha");
    assert_eq!(json["search_query"], "recent decisions");
    assert_eq!(json["search_top_k"], 15);
    // Negative-shape: unset `Option` fields stay absent (never `null`).
    assert!(json.get("search_max_distance").is_none());
    assert!(json.get("include_most_frequent").is_none());
    assert!(json.get("max_conclusions").is_none());
}
