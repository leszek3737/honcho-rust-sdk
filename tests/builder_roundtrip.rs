#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

use std::collections::HashMap;

use honcho_ai::types::conclusion::{
    ConclusionBatchCreate, ConclusionCreate, ConclusionGet, ConclusionQuery,
};
use honcho_ai::types::dream::{DreamType, ScheduleDreamRequest};
use honcho_ai::types::message::{
    MessageConfiguration, MessageCreate, MessageSearchOptions, MessageUpdate,
};
use honcho_ai::types::peer::{
    PeerCardSet, PeerContextOptions, PeerCreate, PeerGet, PeerRepresentationGet, PeerUpdate,
};
use serde_json::json;

/// Serialize through the JSON *text* path and deserialize back, asserting the
/// value survives unchanged.
///
/// Uses `to_string`/`from_str` (not `to_value`/`from_value`) so the string
/// parser is exercised too, and folds the fidelity assert in: a field a serde
/// impl silently drops fails here instead of passing tautologically. Only types
/// that derive `PartialEq` can use this helper.
fn roundtrip<T>(val: &T)
where
    T: serde::Serialize + serde::de::DeserializeOwned + std::fmt::Debug + PartialEq,
{
    let json = serde_json::to_string(val).unwrap();
    let parsed: T = serde_json::from_str(&json).unwrap();
    assert_eq!(
        *val,
        parsed,
        "roundtrip mismatch for {}",
        std::any::type_name::<T>()
    );
}

// ─── Peer types ──────────────────────────────────────────────────────

#[test]
fn peer_create_builder_roundtrip() {
    // NOTE: PeerCreate has no `PartialEq` derive (src, PR5), so the strict
    // single-`assert_eq!` helper is unavailable; assert every field explicitly.
    let metadata = HashMap::from([("role".to_string(), json!("user"))]);
    let config = HashMap::from([("lang".to_string(), json!("en"))]);

    let p = PeerCreate::builder()
        .id("peer-1")
        .metadata(metadata)
        .configuration(config)
        .build();
    // Builder must set every field it was given.
    assert_eq!(p.id, "peer-1");
    assert_eq!(p.metadata.as_ref().unwrap()["role"], json!("user"));
    assert_eq!(p.configuration.as_ref().unwrap()["lang"], json!("en"));

    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json["id"], json!("peer-1"));
    assert_eq!(json["metadata"]["role"], json!("user"));
    assert_eq!(json["configuration"]["lang"], json!("en"));

    // Deserialize back and assert *every* field survives (not just `id`).
    let rt: PeerCreate = serde_json::from_value(json).unwrap();
    assert_eq!(rt.id, "peer-1");
    assert_eq!(rt.metadata.unwrap()["role"], json!("user"));
    assert_eq!(rt.configuration.unwrap()["lang"], json!("en"));
}

#[test]
fn peer_update_builder_empty_roundtrip() {
    // NOTE: PeerUpdate has no `PartialEq` derive (src, PR5) — assert per field.
    let p = PeerUpdate::builder().build();
    assert!(p.metadata.is_none());
    assert!(p.configuration.is_none());

    let json = serde_json::to_value(&p).unwrap();
    // Both fields use skip_serializing_if = "Option::is_none" → empty object.
    assert_eq!(json, json!({}));

    let rt: PeerUpdate = serde_json::from_value(json).unwrap();
    assert!(rt.metadata.is_none());
    assert!(rt.configuration.is_none());
}

#[test]
fn peer_update_builder_populated_roundtrip() {
    // Exercises the `Some` / skip_serializing_if path the empty case can't reach.
    let metadata = HashMap::from([("role".to_string(), json!("admin"))]);
    let config = HashMap::from([("lang".to_string(), json!("fr"))]);

    let p = PeerUpdate::builder()
        .metadata(metadata)
        .configuration(config)
        .build();
    assert_eq!(p.metadata.as_ref().unwrap()["role"], json!("admin"));
    assert_eq!(p.configuration.as_ref().unwrap()["lang"], json!("fr"));

    let json = serde_json::to_value(&p).unwrap();
    // skip_serializing_if must NOT drop populated fields.
    assert_eq!(json["metadata"]["role"], json!("admin"));
    assert_eq!(json["configuration"]["lang"], json!("fr"));

    let rt: PeerUpdate = serde_json::from_value(json).unwrap();
    assert_eq!(rt.metadata.unwrap()["role"], json!("admin"));
    assert_eq!(rt.configuration.unwrap()["lang"], json!("fr"));
}

#[test]
fn peer_get_builder_empty_roundtrip() {
    // NOTE: PeerGet has no `PartialEq` derive (src, PR5) — assert per field.
    let p = PeerGet::builder().build();
    assert!(p.filters.is_none());

    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json, json!({}));
    let rt: PeerGet = serde_json::from_value(json).unwrap();
    assert!(rt.filters.is_none());
}

#[test]
fn peer_get_builder_filters_roundtrip() {
    // The `filters` Some-path was previously untested.
    let filters = HashMap::from([("role".to_string(), json!("user"))]);
    let p = PeerGet::builder().filters(filters).build();
    assert_eq!(p.filters.as_ref().unwrap()["role"], json!("user"));

    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json["filters"]["role"], json!("user"));
    let rt: PeerGet = serde_json::from_value(json).unwrap();
    assert_eq!(rt.filters.unwrap()["role"], json!("user"));
}

#[test]
fn peer_card_set_builder_roundtrip() {
    let p = PeerCardSet::builder()
        .peer_card(vec!["card1".to_string(), "card2".to_string()])
        .build();
    assert_eq!(p.peer_card, vec!["card1", "card2"]);

    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json["peer_card"], json!(["card1", "card2"]));

    roundtrip(&p);
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

    // GATE: PeerRepresentationGet has no `PartialEq` derive (src, PR5), so the
    // builder sets 7 fields but cannot be checked with one `assert_eq!`. Assert
    // every set field instead (the original test checked only 3).
    assert_eq!(p.session_id.as_deref(), Some("sess-1"));
    assert_eq!(p.target.as_deref(), Some("bob"));
    assert_eq!(p.search_query.as_deref(), Some("preferences"));
    assert_eq!(p.search_top_k, Some(10));
    assert_eq!(p.search_max_distance, Some(0.5));
    assert_eq!(p.include_most_frequent, Some(true));
    assert_eq!(p.max_conclusions, Some(25));

    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json["session_id"], json!("sess-1"));
    assert_eq!(json["target"], json!("bob"));
    assert_eq!(json["search_query"], json!("preferences"));
    assert_eq!(json["search_top_k"], json!(10));
    assert_eq!(json["search_max_distance"], json!(0.5));
    assert_eq!(json["include_most_frequent"], json!(true));
    assert_eq!(json["max_conclusions"], json!(25));

    // serde_json round-trips f64 bit-exact → plain equality, no epsilon needed.
    let rt: PeerRepresentationGet = serde_json::from_value(json).unwrap();
    assert_eq!(rt.session_id.as_deref(), Some("sess-1"));
    assert_eq!(rt.target.as_deref(), Some("bob"));
    assert_eq!(rt.search_query.as_deref(), Some("preferences"));
    assert_eq!(rt.search_top_k, Some(10));
    assert_eq!(rt.search_max_distance, Some(0.5));
    assert_eq!(rt.include_most_frequent, Some(true));
    assert_eq!(rt.max_conclusions, Some(25));
}

#[test]
fn peer_context_options_builder_roundtrip() {
    let p = PeerContextOptions::builder()
        .target("bob")
        .search_query("prefs")
        .search_top_k(5)
        .max_conclusions(10)
        .build();
    // Explicit field asserts: roundtrip-equality alone can't catch the builder
    // dropping a field (both ends would be symmetrically `None`).
    assert_eq!(p.target.as_deref(), Some("bob"));
    assert_eq!(p.search_query.as_deref(), Some("prefs"));
    assert_eq!(p.search_top_k, Some(5));
    assert_eq!(p.max_conclusions, Some(10));
    assert_eq!(p.search_max_distance, None);
    assert_eq!(p.include_most_frequent, None);

    roundtrip(&p);
}

// ─── Message types ───────────────────────────────────────────────────

#[test]
fn message_create_builder_roundtrip() {
    let metadata = HashMap::from([("key".to_string(), json!("val"))]);

    let m = MessageCreate::builder()
        .content("hello")
        .peer_id("peer-1")
        .metadata(metadata)
        .build();
    assert_eq!(m.content, "hello");
    assert_eq!(m.peer_id, "peer-1");
    assert_eq!(m.metadata.as_ref().unwrap()["key"], json!("val"));
    // Unset optionals must stay None.
    assert!(m.configuration.is_none());
    assert!(m.created_at.is_none());

    roundtrip(&m);
}

#[test]
fn message_create_builder_full_roundtrip() {
    // Covers the previously-untested `configuration` and `created_at` fields.
    let metadata = HashMap::from([("key".to_string(), json!("val"))]);
    let config: MessageConfiguration =
        serde_json::from_value(json!({ "reasoning": { "enabled": true } })).unwrap();

    let m = MessageCreate::builder()
        .content("hello")
        .peer_id("peer-1")
        .metadata(metadata)
        .configuration(config)
        // `.parse()` infers `DateTime<Utc>` from the setter signature, so the
        // test needs no direct chrono dependency.
        .created_at("2024-01-01T00:00:00Z".parse().unwrap())
        .build();
    assert_eq!(m.content, "hello");
    assert_eq!(m.peer_id, "peer-1");
    assert_eq!(m.metadata.as_ref().unwrap()["key"], json!("val"));
    assert_eq!(
        m.configuration
            .as_ref()
            .unwrap()
            .reasoning
            .as_ref()
            .unwrap()
            .enabled,
        Some(true)
    );
    assert!(m.created_at.is_some());

    let json = serde_json::to_value(&m).unwrap();
    assert_eq!(json["content"], json!("hello"));
    assert_eq!(json["configuration"]["reasoning"]["enabled"], json!(true));
    // `created_at` must serialize (rename intact) as an RFC3339 string.
    assert!(json["created_at"].is_string());

    roundtrip(&m);
}

#[test]
fn message_update_builder_roundtrip() {
    let metadata = HashMap::from([("edited".to_string(), json!(true))]);

    let m = MessageUpdate::builder().metadata(metadata).build();
    assert_eq!(m.metadata.as_ref().unwrap()["edited"], json!(true));

    roundtrip(&m);
}

#[test]
fn message_search_options_builder_roundtrip() {
    let filters = HashMap::from([("peer_id".to_string(), json!("p1"))]);
    let m = MessageSearchOptions::builder()
        .query("search term")
        .filters(filters)
        .limit(5)
        .build();
    assert_eq!(m.query, "search term");
    assert_eq!(m.limit, 5);
    assert_eq!(m.filters.as_ref().unwrap()["peer_id"], json!("p1"));

    let json = serde_json::to_value(&m).unwrap();
    assert_eq!(json["filters"]["peer_id"], json!("p1"));
    assert_eq!(json["limit"], json!(5));

    roundtrip(&m);
}

#[test]
fn message_search_options_builder_defaults_roundtrip() {
    // No `limit`/`filters` set → builder default limit = 10, filters omitted.
    let m = MessageSearchOptions::builder().query("q").build();
    assert_eq!(m.limit, 10);
    assert!(m.filters.is_none());

    let json = serde_json::to_value(&m).unwrap();
    // `limit` has no skip predicate → the default is always emitted.
    assert_eq!(json["limit"], json!(10));
    // `filters` uses skip_serializing_if → absent.
    assert!(json.get("filters").is_none());

    roundtrip(&m);
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
    assert_eq!(c.observer_id, "alice");
    assert_eq!(c.observed_id, "bob");
    assert_eq!(c.session_id.as_deref(), Some("sess-1"));

    roundtrip(&c);
}

#[test]
fn conclusion_batch_create_builder_roundtrip() {
    let items = vec![
        ConclusionCreate::builder()
            .content("item 1")
            .observer_id("a")
            .observed_id("b")
            .build(),
    ];
    let batch = ConclusionBatchCreate::builder().conclusions(items).build();
    assert_eq!(batch.conclusions.len(), 1);
    assert_eq!(batch.conclusions[0].content, "item 1");

    roundtrip(&batch);
}

#[test]
fn conclusion_get_builder_roundtrip() {
    // `filters` is now a free-form `HashMap<String, Value>` mirroring `PeerGet`.
    let mut filters = HashMap::new();
    filters.insert("observer_id".to_owned(), json!("alice"));
    let g = ConclusionGet::builder().filters(filters).build();
    assert_eq!(
        g.filters
            .as_ref()
            .and_then(|m| m.get("observer_id"))
            .and_then(|v| v.as_str()),
        Some("alice")
    );

    roundtrip(&g);
}

#[test]
fn conclusion_query_builder_roundtrip() {
    let mut filters = HashMap::new();
    filters.insert("session_id".to_owned(), json!("sess-1"));
    let q = ConclusionQuery::builder()
        .query("search text")
        .top_k(5)
        .distance(0.8)
        .filters(filters)
        .build();
    assert_eq!(q.query, "search text");
    assert_eq!(q.top_k, 5);
    assert_eq!(q.distance, Some(0.8));
    assert_eq!(
        q.filters
            .as_ref()
            .and_then(|m| m.get("session_id"))
            .and_then(|v| v.as_str()),
        Some("sess-1")
    );

    let json = serde_json::to_value(&q).unwrap();
    assert_eq!(json["top_k"], json!(5));
    assert_eq!(json["distance"], json!(0.8));

    roundtrip(&q);
}

#[test]
fn conclusion_query_builder_default_top_k_golden() {
    // No `top_k` set → builder default 10. `top_k` has NO skip predicate, so the
    // default value must still appear on the wire (golden-JSON contract).
    let q = ConclusionQuery::builder().query("hi").build();
    assert_eq!(q.top_k, 10);
    assert!(q.distance.is_none());
    assert!(q.filters.is_none());

    let json = serde_json::to_value(&q).unwrap();
    assert_eq!(json["top_k"], json!(10));
    // `distance` / `filters` use skip_serializing_if → absent.
    assert!(json.get("distance").is_none());
    assert!(json.get("filters").is_none());

    roundtrip(&q);
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
    assert_eq!(req.dream_type, DreamType::Omni);
    assert_eq!(req.observed.as_deref(), Some("bob"));
    assert_eq!(req.session_id.as_deref(), Some("sess-1"));

    let json = serde_json::to_value(&req).unwrap();
    // Golden wire-format: a bad rename of `DreamType::Omni` is caught here.
    assert_eq!(json["dream_type"], json!("omni"));
    assert_eq!(json["observer"], json!("alice"));

    roundtrip(&req);
}
