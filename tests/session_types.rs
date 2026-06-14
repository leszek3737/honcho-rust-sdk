#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

mod common;

use std::collections::HashMap;

use serde_json::{Value, json};

use common::{load_fixture, roundtrip, validate_openapi};
use honcho_ai::types::session::{
    ReasoningConfiguration, SessionConfiguration, SessionContext, SessionContextOptions,
    SessionCreate, SessionGet, SessionPage, SessionPeerConfig, SessionQueueStatus, SessionResponse,
    SessionSummaries, SessionUpdate, Summary, SummaryConfiguration, SummaryType,
};

/// Generates `validate_{min,max}` (SDK *output* vs `OpenAPI` schema) and strict
/// `roundtrip_{min,max}` for a type present in the `OpenAPI` spec.
///
/// `validate_*` deserializes the fixture into the SDK type and validates what
/// the SDK *serializes back* — not the raw fixture — so a divergent
/// `#[serde(rename)]` or dropped field fails the schema check.
///
/// Both round-trips use the strict [`common::roundtrip`] (full fidelity).
/// Fixtures whose `{}`-style defaults are *not* a serialization fixed point are
/// pinned by dedicated golden tests instead (see
/// `session_context_options_min_materializes_defaults`).
macro_rules! schema_tests {
    ($name:ident, $schema:expr, $ty:ty) => {
        mod $name {
            use super::*;

            #[test]
            fn validate_min() {
                let fixture = load_fixture($schema, "min");
                let value: $ty = serde_json::from_value(fixture).unwrap();
                let output = serde_json::to_value(&value).unwrap();
                validate_openapi(&output, $schema);
            }

            #[test]
            fn validate_max() {
                let fixture = load_fixture($schema, "max");
                let value: $ty = serde_json::from_value(fixture).unwrap();
                let output = serde_json::to_value(&value).unwrap();
                validate_openapi(&output, $schema);
            }

            #[test]
            fn roundtrip_min() {
                roundtrip::<$ty>(load_fixture($schema, "min"));
            }

            #[test]
            fn roundtrip_max() {
                roundtrip::<$ty>(load_fixture($schema, "max"));
            }
        }
    };
}

schema_tests!(session_create, "SessionCreate", SessionCreate);
schema_tests!(session_update, "SessionUpdate", SessionUpdate);
schema_tests!(session_get, "SessionGet", SessionGet);
schema_tests!(
    session_configuration,
    "SessionConfiguration",
    SessionConfiguration
);
schema_tests!(session_context, "SessionContext", SessionContext);
schema_tests!(session_peer_config, "SessionPeerConfig", SessionPeerConfig);
schema_tests!(
    session_queue_status,
    "SessionQueueStatus",
    SessionQueueStatus
);
schema_tests!(session_summaries, "SessionSummaries", SessionSummaries);
schema_tests!(summary, "Summary", Summary);
schema_tests!(
    summary_configuration,
    "SummaryConfiguration",
    SummaryConfiguration
);

// The `Session/min` and `Page_Session_/max` fixtures were trimmed to the
// SDK-owned shape (no `metadata: {}`): `SessionResponse::metadata` is
// `skip_serializing_if = "HashMap::is_empty"`, so an empty map is omitted on
// output and the explicit `{}` was the *only* thing preventing strict fidelity.
// The retained `configuration: {}` is symmetric (default `SessionConfiguration`
// re-serializes to `{}`), so both variants now run the strict round-trip.
// `metadata` is not required by the `Session` OpenAPI schema, so `validate_*`
// still passes against the SDK output.
schema_tests!(session, "Session", SessionResponse);
schema_tests!(page_session, "Page_Session_", SessionPage);

// `SessionContextOptions` is intentionally absent from the OpenAPI spec (it is a
// client-side query helper), so it gets round-trip coverage only. `max` is
// strict — comparing the whole `Value` catches any leaked extra key or
// mis-renamed field. The `min` fixture (`{}`) is *not* a fixed point: `summary`
// / `limit_to_session` carry serde defaults with no `skip_serializing_if`, so
// `{}` materializes them on output. It is therefore covered by the dedicated
// golden test `session_context_options_min_materializes_defaults` below instead
// of a round-trip, which pins the exact canonical shape rather than mere
// idempotence.
mod session_context_options {
    use super::*;

    #[test]
    fn roundtrip_max() {
        roundtrip::<SessionContextOptions>(load_fixture("SessionContextOptions", "max"));
    }
}

#[test]
fn session_create_builder_minimal() {
    let created = SessionCreate::builder().id("test-session").build();
    let json = serde_json::to_value(&created).unwrap();
    assert_eq!(json["id"], "test-session");
    assert!(json.get("metadata").is_none());
    assert!(json.get("peers").is_none());
    assert!(json.get("configuration").is_none());
}

#[test]
fn session_create_builder_full() {
    // Typed inputs (built from `Default` + field assignment, since these types
    // are `#[non_exhaustive]` and reject struct literals from this external test
    // crate): type-checked, no JSON round-trip on the way in.
    let metadata: HashMap<String, Value> = HashMap::from([("env".to_string(), json!("test"))]);

    let mut peer_a = SessionPeerConfig::default();
    peer_a.observe_me = Some(true);
    peer_a.observe_others = Some(false);
    let peers = HashMap::from([("peer_a".to_string(), peer_a)]);

    let mut reasoning = ReasoningConfiguration::default();
    reasoning.enabled = Some(true);
    let mut configuration = SessionConfiguration::default();
    configuration.reasoning = Some(reasoning);

    let created = SessionCreate::builder()
        .id("full-session")
        .metadata(metadata)
        .peers(peers)
        .configuration(configuration)
        .build();

    // Whole-`Value` assert: covers `metadata` and `observe_others: false` (both
    // previously set-but-unasserted) and fails on any leaked or dropped key.
    let json = serde_json::to_value(&created).unwrap();
    assert_eq!(
        json,
        json!({
            "id": "full-session",
            "metadata": {"env": "test"},
            "peers": {"peer_a": {"observe_me": true, "observe_others": false}},
            "configuration": {"reasoning": {"enabled": true}}
        })
    );
}

#[test]
fn session_update_builder_skips_none() {
    let update = SessionUpdate::builder().build();
    let json = serde_json::to_value(&update).unwrap();
    assert_eq!(json, json!({}));
}

#[test]
fn session_update_builder_full() {
    let metadata: HashMap<String, Value> = HashMap::from([("updated".to_string(), json!(true))]);

    let mut reasoning = ReasoningConfiguration::default();
    reasoning.enabled = Some(false);
    let mut configuration = SessionConfiguration::default();
    configuration.reasoning = Some(reasoning);

    let update = SessionUpdate::builder()
        .metadata(metadata)
        .configuration(configuration)
        .build();

    let json = serde_json::to_value(&update).unwrap();
    assert_eq!(
        json,
        json!({
            "metadata": {"updated": true},
            "configuration": {"reasoning": {"enabled": false}}
        })
    );
}

#[test]
fn session_get_builder_skips_none() {
    let get = SessionGet::builder().build();
    let json = serde_json::to_value(&get).unwrap();
    assert_eq!(json, json!({}));
}

#[test]
fn session_get_builder_full() {
    let filters: HashMap<String, Value> = HashMap::from([
        ("is_active".to_string(), json!(true)),
        ("tag".to_string(), json!("production")),
    ]);

    let get = SessionGet::builder().filters(filters).build();

    let json = serde_json::to_value(&get).unwrap();
    assert_eq!(
        json,
        json!({
            "filters": {"is_active": true, "tag": "production"}
        })
    );
}

// ── forward-compatibility ────────────────────────────────────────────

#[test]
fn unknown_summary_type_deserializes_to_unknown() {
    // A summary type added server-side must not fail deserialization.
    let json = json!({
        "content": "x",
        "message_id": "m0",
        "summary_type": "medium",
        "created_at": "2025-01-15T10:30:00Z",
        "token_count": 1
    });
    let summary: Summary = serde_json::from_value(json).unwrap();
    assert_eq!(summary.summary_type, SummaryType::Unknown);
}

#[test]
fn session_context_with_unknown_summary_type_deserializes() {
    // The whole SessionContext must deserialize even with an unknown summary type.
    let json = json!({
        "id": "s1",
        "messages": [],
        "summary": {
            "content": "x",
            "message_id": "m0",
            "summary_type": "brand_new_kind",
            "created_at": "2025-01-15T10:30:00Z",
            "token_count": 1
        }
    });
    let ctx: SessionContext = serde_json::from_value(json).unwrap();
    assert_eq!(ctx.summary.unwrap().summary_type, SummaryType::Unknown);
}

#[test]
fn known_summary_types_still_deserialize() {
    let short: Summary = serde_json::from_value(json!({
        "content": "x", "message_id": "m0", "summary_type": "short",
        "created_at": "2025-01-15T10:30:00Z", "token_count": 1
    }))
    .unwrap();
    assert_eq!(short.summary_type, SummaryType::Short);
    let long: Summary = serde_json::from_value(json!({
        "content": "x", "message_id": "m0", "summary_type": "long",
        "created_at": "2025-01-15T10:30:00Z", "token_count": 1
    }))
    .unwrap();
    assert_eq!(long.summary_type, SummaryType::Long);
}

#[test]
fn session_peer_config_accepts_unknown_fields() {
    // `deny_unknown_fields` removed: forward-compatible with new server fields.
    let json = json!({
        "observe_me": true,
        "future_field": 123
    });
    let cfg: SessionPeerConfig = serde_json::from_value(json).unwrap();
    assert_eq!(cfg.observe_me, Some(true));
    assert_eq!(cfg.observe_others, None);
}

#[test]
fn session_context_options_defaults() {
    let opts: SessionContextOptions = serde_json::from_value(json!({})).unwrap();
    assert!(opts.summary);
    assert!(!opts.limit_to_session);
    assert!(opts.tokens.is_none());
    assert!(opts.peer_target.is_none());
    assert!(opts.peer_perspective.is_none());
    assert!(opts.search_query.is_none());
    assert!(opts.search_top_k.is_none());
    assert!(opts.search_max_distance.is_none());
    assert!(opts.include_most_frequent.is_none());
    assert!(opts.max_conclusions.is_none());
}

#[test]
fn session_context_options_min_materializes_defaults() {
    // The `min` fixture is `{}`, but `summary` (`default = "default_true"`) and
    // `limit_to_session` (`#[serde(default)]`) have NO `skip_serializing_if`, so
    // deserializing `{}` and re-serializing *materializes* those two defaults
    // (the deliberate "always send the value, never make the server infer intent
    // from omission" stance shared with `SessionListOptions::page/size`). The
    // bare `{}` fixture is therefore not a fixed point, so we pin the exact
    // canonical output instead of a round-trip. All `Option` fields stay omitted.
    let opts: SessionContextOptions =
        serde_json::from_value(load_fixture("SessionContextOptions", "min")).unwrap();
    let json = serde_json::to_value(&opts).unwrap();
    assert_eq!(json, json!({ "summary": true, "limit_to_session": false }));
}
