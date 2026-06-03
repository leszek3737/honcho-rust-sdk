#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

use std::collections::HashMap;

use honcho_ai::types::conclusion::{
    ConclusionBatchCreate, ConclusionCreate, ConclusionFilters, ConclusionGet, ConclusionQuery,
};
use honcho_ai::types::dream::{DreamType, ScheduleDreamRequest};
use honcho_ai::types::message::{MessageCreate, MessageSearchOptions, MessageUpdate};
use honcho_ai::types::peer::{
    PeerCardSet, PeerContextOptions, PeerCreate, PeerGet, PeerRepresentationGet, PeerUpdate,
};
use serde_json::json;

fn roundtrip<T: serde::Serialize + serde::de::DeserializeOwned + std::fmt::Debug + PartialEq>(
    val: &T,
) -> T {
    let json = serde_json::to_value(val).unwrap();
    serde_json::from_value(json).unwrap()
}

// ─── Peer types ──────────────────────────────────────────────────────

#[test]
fn peer_create_builder_roundtrip() {
    let mut metadata = HashMap::new();
    metadata.insert("role".to_string(), json!("user"));
    let mut config = HashMap::new();
    config.insert("lang".to_string(), json!("en"));

    let p = PeerCreate::builder()
        .id("peer-1")
        .metadata(metadata)
        .configuration(config)
        .build();
    assert_eq!(p.id, "peer-1");

    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json["id"], json!("peer-1"));
    assert_eq!(json["metadata"]["role"], json!("user"));
    assert_eq!(json["configuration"]["lang"], json!("en"));
    let rt: PeerCreate = serde_json::from_value(json).unwrap();
    assert_eq!(rt.id, "peer-1");
}

#[test]
fn peer_update_builder_roundtrip() {
    let p = PeerUpdate::builder().build();
    assert!(p.metadata.is_none());
    assert!(p.configuration.is_none());

    let json = serde_json::to_value(&p).unwrap();
    let rt: PeerUpdate = serde_json::from_value(json).unwrap();
    assert!(rt.metadata.is_none());
    assert!(rt.configuration.is_none());
}

#[test]
fn peer_get_builder_roundtrip() {
    let p = PeerGet::builder().build();
    assert!(p.filters.is_none());

    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json, json!({}));
    let rt: PeerGet = serde_json::from_value(json).unwrap();
    assert!(rt.filters.is_none());
}

#[test]
fn peer_card_set_builder_roundtrip() {
    let p = PeerCardSet::builder()
        .peer_card(vec!["card1".to_string(), "card2".to_string()])
        .build();
    assert_eq!(p.peer_card, vec!["card1", "card2"]);

    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json["peer_card"], json!(["card1", "card2"]));
    let rt: PeerCardSet = serde_json::from_value(json).unwrap();
    assert_eq!(rt.peer_card, vec!["card1".to_string(), "card2".to_string()]);
}

#[test]
fn peer_representation_get_builder_roundtrip() {
    let p = PeerRepresentationGet::builder()
        .session_id("sess-1")
        .target("bob")
        .search_query("preferences")
        .search_top_k(10)
        .search_max_distance(0.5)
        .include_most_frequent(true)
        .max_conclusions(25)
        .build();
    assert_eq!(p.session_id.as_deref(), Some("sess-1"));

    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json["session_id"], json!("sess-1"));
    assert_eq!(json["search_top_k"], json!(10));
    assert_eq!(json["search_max_distance"], json!(0.5));
    let rt: PeerRepresentationGet = serde_json::from_value(json).unwrap();
    assert_eq!(rt.session_id.as_deref(), Some("sess-1"));
    assert_eq!(rt.search_top_k, Some(10));
    assert!((rt.search_max_distance.unwrap() - 0.5).abs() < f64::EPSILON);
}

#[test]
fn peer_context_options_builder_roundtrip() {
    let p = PeerContextOptions::builder()
        .target("bob")
        .search_query("prefs")
        .search_top_k(5)
        .max_conclusions(10)
        .build();
    assert_eq!(p.target.as_deref(), Some("bob"));

    let rt = roundtrip(&p);
    assert_eq!(p, rt);
}

// ─── Message types ───────────────────────────────────────────────────

#[test]
fn message_create_builder_roundtrip() {
    let mut metadata = HashMap::new();
    metadata.insert("key".to_string(), json!("val"));

    let m = MessageCreate::builder()
        .content("hello")
        .peer_id("peer-1")
        .metadata(metadata)
        .build();
    assert_eq!(m.content, "hello");
    assert_eq!(m.peer_id, "peer-1");

    let rt = roundtrip(&m);
    assert_eq!(m, rt);
}

#[test]
fn message_update_builder_roundtrip() {
    let mut metadata = HashMap::new();
    metadata.insert("edited".to_string(), json!(true));

    let m = MessageUpdate::builder().metadata(metadata).build();
    assert_eq!(m.metadata.as_ref().unwrap()["edited"], json!(true));

    let rt = roundtrip(&m);
    assert_eq!(m, rt);
}

#[test]
fn message_search_options_builder_roundtrip() {
    let m = MessageSearchOptions::builder()
        .query("search term")
        .limit(5)
        .build();
    assert_eq!(m.query, "search term");
    assert_eq!(m.limit, 5);

    let rt = roundtrip(&m);
    assert_eq!(m, rt);
}

// ─── Conclusion types ────────────────────────────────────────────────

#[test]
fn conclusion_create_builder_roundtrip() {
    let c = ConclusionCreate::builder()
        .content("test conclusion")
        .observer_id("alice")
        .observed_id("bob")
        .session_id("sess-1")
        .build();
    assert_eq!(c.content, "test conclusion");

    let rt = roundtrip(&c);
    assert_eq!(c, rt);
}

#[test]
fn conclusion_batch_create_builder_roundtrip() {
    let items = vec![ConclusionCreate::builder()
        .content("item 1")
        .observer_id("a")
        .observed_id("b")
        .build()];
    let batch = ConclusionBatchCreate::builder().conclusions(items).build();
    assert_eq!(batch.conclusions.len(), 1);

    let rt = roundtrip(&batch);
    assert_eq!(batch, rt);
}

#[test]
fn conclusion_filters_builder_roundtrip() {
    let f = ConclusionFilters::builder()
        .observer_id("alice")
        .observed_id("bob")
        .session_id("sess-1")
        .build();
    assert_eq!(f.observer_id.as_deref(), Some("alice"));

    let rt = roundtrip(&f);
    assert_eq!(f, rt);
}

#[test]
fn conclusion_get_builder_roundtrip() {
    let filters = ConclusionFilters::builder()
        .observer_id("alice")
        .build();
    let g = ConclusionGet::builder().filters(filters).build();
    assert_eq!(
        g.filters.as_ref().unwrap().observer_id.as_deref(),
        Some("alice")
    );

    let rt = roundtrip(&g);
    assert_eq!(g, rt);
}

#[test]
fn conclusion_query_builder_roundtrip() {
    let filters = ConclusionFilters::builder()
        .session_id("sess-1")
        .build();
    let q = ConclusionQuery::builder()
        .query("search text")
        .top_k(5)
        .distance(0.8)
        .filters(filters)
        .build();
    assert_eq!(q.query, "search text");
    assert_eq!(q.top_k, 5);

    let rt = roundtrip(&q);
    assert_eq!(q, rt);
}

// ─── Dream types ─────────────────────────────────────────────────────

#[test]
fn schedule_dream_request_builder_roundtrip() {
    let req = ScheduleDreamRequest::builder()
        .observer("alice")
        .dream_type(DreamType::Omni)
        .observed("bob")
        .session_id("sess-1")
        .build();
    assert_eq!(req.observer, "alice");

    let rt = roundtrip(&req);
    assert_eq!(req, rt);
}
