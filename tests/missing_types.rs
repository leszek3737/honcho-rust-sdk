#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

mod common;
use common::{load_fixture, roundtrip};

use honcho_ai::types::conclusion::ConclusionFilters;
use honcho_ai::types::peer::PeerContextOptions;
use honcho_ai::types::session::SessionListOptions;
use serde_json::json;

// --- ConclusionFilters (has Deserialize + builder) ---

#[test]
fn conclusion_filters_roundtrip_min() {
    roundtrip::<ConclusionFilters>(load_fixture("ConclusionFilters", "min"));
}

#[test]
fn conclusion_filters_roundtrip_max() {
    roundtrip::<ConclusionFilters>(load_fixture("ConclusionFilters", "max"));
}

#[test]
fn conclusion_filters_empty() {
    let f = ConclusionFilters::builder().build();
    let json = serde_json::to_value(&f).unwrap();
    assert_eq!(json, json!({}));
}

#[test]
fn conclusion_filters_partial() {
    let f = ConclusionFilters::builder()
        .observer_id("peer1".to_string())
        .build();
    let json = serde_json::to_value(&f).unwrap();
    assert_eq!(json["observer_id"], "peer1");
    assert!(json.get("observed_id").is_none());
    assert!(json.get("session_id").is_none());
}

// --- SessionListOptions (has Deserialize + builder) ---

#[test]
fn session_list_options_roundtrip_min() {
    roundtrip::<SessionListOptions>(load_fixture("SessionListOptions", "min"));
}

#[test]
fn session_list_options_roundtrip_max() {
    roundtrip::<SessionListOptions>(load_fixture("SessionListOptions", "max"));
}

#[test]
fn session_list_options_defaults() {
    let opts: SessionListOptions = serde_json::from_value(json!({})).unwrap();
    assert_eq!(opts.page, 1);
    assert_eq!(opts.size, 50);
    assert!(!opts.reverse);
    assert!(opts.filters.is_none());
}

#[test]
fn session_list_options_builder() {
    let opts = SessionListOptions::builder()
        .page(2u64)
        .size(25u64)
        .reverse(true)
        .build();
    let json = serde_json::to_value(&opts).unwrap();
    assert_eq!(json["page"], 2);
    assert_eq!(json["size"], 25);
    assert_eq!(json["reverse"], true);
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
