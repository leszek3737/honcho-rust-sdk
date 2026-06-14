#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

mod common;

use common::{load_fixture, roundtrip, validate_openapi};
use honcho_ai::types::session::{
    SessionConfiguration, SessionContext, SessionContextOptions, SessionCreate, SessionGet,
    SessionPage, SessionPeerConfig, SessionQueueStatus, SessionResponse, SessionSummaries,
    SessionUpdate, Summary, SummaryConfiguration, SummaryType,
};

macro_rules! schema_tests {
    ($name:ident, $schema:expr, $ty:ty) => {
        mod $name {
            use super::*;

            #[test]
            fn validate_min() {
                let fixture = load_fixture($schema, "min");
                validate_openapi(fixture, $schema);
            }

            #[test]
            fn validate_max() {
                let fixture = load_fixture($schema, "max");
                validate_openapi(fixture, $schema);
            }

            #[test]
            fn roundtrip_min() {
                let fixture = load_fixture($schema, "min");
                roundtrip::<$ty>(fixture);
            }

            #[test]
            fn roundtrip_max() {
                let fixture = load_fixture($schema, "max");
                roundtrip::<$ty>(fixture);
            }
        }
    };
}

schema_tests!(session, "Session", SessionResponse);
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
schema_tests!(page_session, "Page_Session_", SessionPage);

#[test]
fn session_create_builder_minimal() {
    let created = SessionCreate::builder()
        .id("test-session".to_string())
        .build();
    let json = serde_json::to_value(&created).unwrap();
    assert_eq!(json["id"], "test-session");
    assert!(json.get("metadata").is_none());
    assert!(json.get("peers").is_none());
    assert!(json.get("configuration").is_none());
}

#[test]
fn session_create_builder_full() {
    let peers_json = serde_json::json!({
        "peer_a": {"observe_me": true, "observe_others": false}
    });
    let peers: std::collections::HashMap<String, SessionPeerConfig> =
        serde_json::from_value(peers_json).unwrap();
    let config_json = serde_json::json!({
        "reasoning": {"enabled": true}
    });
    let config: SessionConfiguration = serde_json::from_value(config_json).unwrap();

    let created = SessionCreate::builder()
        .id("full-session".to_string())
        .metadata(serde_json::from_value(serde_json::json!({"env": "test"})).unwrap())
        .peers(peers)
        .configuration(config)
        .build();
    let json = serde_json::to_value(&created).unwrap();
    assert_eq!(json["id"], "full-session");
    assert_eq!(json["peers"]["peer_a"]["observe_me"], true);
    assert_eq!(json["configuration"]["reasoning"]["enabled"], true);
}

#[test]
fn session_update_builder_skips_none() {
    let update = SessionUpdate::builder().build();
    let json = serde_json::to_value(&update).unwrap();
    assert_eq!(json, serde_json::json!({}));
}

#[test]
fn session_get_builder_skips_none() {
    let get = SessionGet::builder().build();
    let json = serde_json::to_value(&get).unwrap();
    assert_eq!(json, serde_json::json!({}));
}

#[test]
fn session_context_options_roundtrip_min() {
    let fixture = load_fixture("SessionContextOptions", "min");
    roundtrip::<SessionContextOptions>(fixture);
}

#[test]
fn session_context_options_roundtrip_max() {
    let fixture = load_fixture("SessionContextOptions", "max");
    roundtrip::<SessionContextOptions>(fixture);
}

// ── forward-compatibility ────────────────────────────────────────────

#[test]
fn unknown_summary_type_deserializes_to_unknown() {
    // A summary type added server-side must not fail deserialization.
    let json = serde_json::json!({
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
    let json = serde_json::json!({
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
    let short: Summary = serde_json::from_value(serde_json::json!({
        "content": "x", "message_id": "m0", "summary_type": "short",
        "created_at": "2025-01-15T10:30:00Z", "token_count": 1
    }))
    .unwrap();
    assert_eq!(short.summary_type, SummaryType::Short);
    let long: Summary = serde_json::from_value(serde_json::json!({
        "content": "x", "message_id": "m0", "summary_type": "long",
        "created_at": "2025-01-15T10:30:00Z", "token_count": 1
    }))
    .unwrap();
    assert_eq!(long.summary_type, SummaryType::Long);
}

#[test]
fn session_peer_config_accepts_unknown_fields() {
    // `deny_unknown_fields` removed: forward-compatible with new server fields.
    let json = serde_json::json!({
        "observe_me": true,
        "future_field": 123
    });
    let cfg: SessionPeerConfig = serde_json::from_value(json).unwrap();
    assert_eq!(cfg.observe_me, Some(true));
    assert_eq!(cfg.observe_others, None);
}

#[test]
fn session_context_options_defaults() {
    let opts: SessionContextOptions = serde_json::from_value(serde_json::json!({})).unwrap();
    assert!(opts.summary);
    assert!(!opts.limit_to_session);
    assert!(opts.tokens.is_none());
    assert!(opts.peer_target.is_none());
    assert!(opts.peer_perspective.is_none());
}
