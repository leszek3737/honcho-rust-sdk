#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

mod common;

use std::collections::HashMap;

use common::{load_fixture, roundtrip, validate_openapi};
use honcho_ai::types::workspace::{
    DreamConfiguration, PeerCardConfiguration, ReasoningConfiguration, SummaryConfiguration,
    Workspace, WorkspaceConfiguration, WorkspaceConfigurationSet, WorkspaceCreate, WorkspaceGet,
    WorkspaceMetadataSet, WorkspacePage, WorkspaceSearchRequest, WorkspaceUpdate,
};
use serde_json::{Value, json};

/// Generates the four canonical fixture tests for a `(schema, type)` pair.
///
/// `*_validates` deserializes the fixture into the SDK type and validates the
/// SDK's *serialized output* against the `OpenAPI` schema — not the raw fixture —
/// so a type that diverges from its schema (bad `#[serde(rename)]`, dropped
/// field) is actually exercised. `*_roundtrip` delegates to the strict helper
/// (`canonicalize(fixture) == canonicalize(sdk_output)`).
macro_rules! schema_test {
    ($name:ident, $schema:literal, $type:ty) => {
        mod $name {
            use super::*;

            #[test]
            fn min_validates() {
                let fixture = load_fixture($schema, "min");
                let value: $type = serde_json::from_value(fixture).unwrap();
                let output = serde_json::to_value(&value).unwrap();
                validate_openapi(&output, $schema);
            }

            #[test]
            fn max_validates() {
                let fixture = load_fixture($schema, "max");
                let value: $type = serde_json::from_value(fixture).unwrap();
                let output = serde_json::to_value(&value).unwrap();
                validate_openapi(&output, $schema);
            }

            #[test]
            fn min_roundtrip() {
                let fixture = load_fixture($schema, "min");
                roundtrip::<$type>(fixture);
            }

            #[test]
            fn max_roundtrip() {
                let fixture = load_fixture($schema, "max");
                roundtrip::<$type>(fixture);
            }
        }
    };
}

schema_test!(workspace, "Workspace", Workspace);
schema_test!(workspace_create, "WorkspaceCreate", WorkspaceCreate);
schema_test!(workspace_update, "WorkspaceUpdate", WorkspaceUpdate);
schema_test!(
    workspace_configuration,
    "WorkspaceConfiguration",
    WorkspaceConfiguration
);
schema_test!(workspace_get, "WorkspaceGet", WorkspaceGet);
schema_test!(workspace_page, "Page_Workspace_", WorkspacePage);
schema_test!(
    reasoning_config,
    "ReasoningConfiguration",
    ReasoningConfiguration
);
schema_test!(
    peer_card_config,
    "PeerCardConfiguration",
    PeerCardConfiguration
);
schema_test!(summary_config, "SummaryConfiguration", SummaryConfiguration);
schema_test!(dream_config, "DreamConfiguration", DreamConfiguration);

// WorkspaceSearchRequest is an inline request body with no named OpenAPI schema,
// so only the serde roundtrip is asserted (no OpenAPI validation).
#[test]
fn workspace_search_request_min_roundtrip() {
    let fixture = load_fixture("WorkspaceSearchRequest", "min");
    roundtrip::<WorkspaceSearchRequest>(fixture);
}

#[test]
fn workspace_search_request_max_roundtrip() {
    let fixture = load_fixture("WorkspaceSearchRequest", "max");
    roundtrip::<WorkspaceSearchRequest>(fixture);
}

// ---------------------------------------------------------------------------
// Builder — positive cases
// ---------------------------------------------------------------------------

#[test]
fn workspace_builder_minimal() {
    let body = WorkspaceCreate::builder().id("test-ws").build();
    let json = serde_json::to_value(&body).unwrap();
    // `metadata`/`configuration` are `Option` + `skip_serializing_if`, so a
    // minimal create must emit *exactly* `{"id": ".."}` — no `null` keys. Full
    // shape assert: a regression that leaks `metadata: null` fails here.
    assert_eq!(json, json!({ "id": "test-ws" }));
}

#[test]
fn workspace_create_builder_full() {
    let metadata: HashMap<String, Value> = HashMap::from([("env".to_owned(), json!("staging"))]);
    let configuration: WorkspaceConfiguration =
        serde_json::from_value(json!({ "reasoning": { "enabled": true } })).unwrap();

    let body = WorkspaceCreate::builder()
        .id("full-ws")
        .metadata(metadata)
        .configuration(configuration)
        .build();

    let json = serde_json::to_value(&body).unwrap();
    assert_eq!(
        json,
        json!({
            "id": "full-ws",
            "metadata": { "env": "staging" },
            "configuration": { "reasoning": { "enabled": true } }
        })
    );
}

#[test]
fn workspace_get_builder_with_filters() {
    let filters: HashMap<String, Value> = HashMap::from([("team".to_owned(), json!("core"))]);
    let body = WorkspaceGet::builder().filters(filters).build();
    let json = serde_json::to_value(&body).unwrap();
    assert_eq!(json, json!({ "filters": { "team": "core" } }));
}

#[test]
fn workspace_update_builder_empty_skips_all() {
    let body = WorkspaceUpdate::builder().build();
    let json = serde_json::to_value(&body).unwrap();
    assert!(json.as_object().unwrap().is_empty());
}

#[test]
fn workspace_get_builder_empty_skips_filters() {
    let body = WorkspaceGet::builder().build();
    let json = serde_json::to_value(&body).unwrap();
    assert!(json.as_object().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// WorkspaceUpdate — double-`Option` three-state semantics
//
// `Option<Option<T>>` with `#[serde(with = "double_option")]`:
//   - outer `None`        (omitted)     -> field skipped on the wire
//   - `Some(None)`        (explicit)    -> JSON `null` (clear/reset)
//   - `Some(Some(v))`     (explicit)    -> the value (overwrite)
// `Some(None)` (explicit null != omission) is serde's most bug-prone case; the
// builder, the wire encoding, and the decode path are all gated below.
// ---------------------------------------------------------------------------

#[test]
fn workspace_update_builder_some_some_overwrites() {
    let metadata: HashMap<String, Value> = HashMap::from([("updated".to_owned(), json!(true))]);
    // `.metadata(Some(map))` sets the outer+inner `Some(Some(_))` (overwrite).
    let body = WorkspaceUpdate::builder().metadata(Some(metadata)).build();
    let json = serde_json::to_value(&body).unwrap();
    assert_eq!(json, json!({ "metadata": { "updated": true } }));
}

#[test]
fn workspace_update_builder_some_none_emits_explicit_null() {
    // `.metadata(None)` sets the outer `Some(None)` — an explicit clear — which
    // must serialize to a literal `null`, *not* be skipped like an omission.
    let body = WorkspaceUpdate::builder()
        .metadata(None::<HashMap<String, Value>>)
        .build();
    let json = serde_json::to_value(&body).unwrap();
    assert_eq!(json, json!({ "metadata": null }));
}

#[test]
fn workspace_update_explicit_null_decodes_to_some_none() {
    // The wire `null` must decode back to `Some(None)` (explicit clear), while
    // the omitted `configuration` stays the outer `None` (leave unchanged).
    let body: WorkspaceUpdate = serde_json::from_value(json!({ "metadata": null })).unwrap();
    assert_eq!(body.metadata, Some(None));
    assert_eq!(body.configuration, None);

    // Symmetry: re-serializing yields the same explicit `null`.
    let json = serde_json::to_value(&body).unwrap();
    assert_eq!(json, json!({ "metadata": null }));
}

#[test]
fn workspace_update_omitted_field_is_outer_none() {
    // An absent field decodes to the outer `None` (leave unchanged), distinct
    // from the explicit-`null` `Some(None)` clear above.
    let body: WorkspaceUpdate = serde_json::from_value(json!({})).unwrap();
    assert_eq!(body.metadata, None);
    assert_eq!(body.configuration, None);
}

// ---------------------------------------------------------------------------
// Workspace.configuration is NOT `skip_serializing_if` — it always emits
// ---------------------------------------------------------------------------

#[test]
fn workspace_configuration_is_always_emitted() {
    // `Workspace.configuration` has `#[serde(default)]` but *no*
    // `skip_serializing_if`, so a default (empty) configuration still serializes
    // to `configuration: {}` rather than being omitted. Lock that contract.
    let ws: Workspace = serde_json::from_value(json!({
        "id": "ws-no-config",
        "created_at": "2025-01-15T10:30:00Z"
    }))
    .unwrap();
    let json = serde_json::to_value(&ws).unwrap();
    assert_eq!(json["configuration"], json!({}));
    // `metadata` *is* `skip_serializing_if = HashMap::is_empty`, so an empty map
    // is omitted — the two fields must behave differently.
    assert!(json.get("metadata").is_none());
}

// ---------------------------------------------------------------------------
// Omitted-from-OpenAPI request bodies — serde coverage
// ---------------------------------------------------------------------------

#[test]
fn workspace_metadata_set_roundtrips() {
    let fixture = json!({ "metadata": { "tier": "gold", "seats": 5 } });
    let value: WorkspaceMetadataSet = serde_json::from_value(fixture.clone()).unwrap();
    assert_eq!(value.metadata["tier"], json!("gold"));
    assert_eq!(serde_json::to_value(&value).unwrap(), fixture);
}

#[test]
fn workspace_configuration_set_roundtrips() {
    let fixture = json!({ "configuration": { "reasoning": { "enabled": false } } });
    let value: WorkspaceConfigurationSet = serde_json::from_value(fixture.clone()).unwrap();
    assert_eq!(serde_json::to_value(&value).unwrap(), fixture);
}

#[test]
fn workspace_search_request_value_asserts() {
    let fixture = load_fixture("WorkspaceSearchRequest", "max");
    let req: WorkspaceSearchRequest = serde_json::from_value(fixture).unwrap();
    assert_eq!(req.query, "search term");
    assert_eq!(req.limit, 50);
    assert_eq!(req.filters.as_ref().unwrap()["env"], json!("prod"));
}

#[test]
fn peer_card_config_use_rename_is_wired() {
    // `PeerCardConfiguration.use_peer_card` is `#[serde(rename = "use")]` (a
    // reserved keyword). Assert the wire key is `use`, never `use_peer_card`.
    let cfg: PeerCardConfiguration =
        serde_json::from_value(json!({ "use": true, "create": false })).unwrap();
    assert_eq!(cfg.use_peer_card, Some(true));
    let json = serde_json::to_value(&cfg).unwrap();
    assert_eq!(json, json!({ "use": true, "create": false }));
    assert!(json.get("use_peer_card").is_none());
}

// ---------------------------------------------------------------------------
// Negative serde cases — assert the *cause*, not bare `is_err()`
// ---------------------------------------------------------------------------

#[test]
fn workspace_rejects_bad_created_at() {
    let err = serde_json::from_value::<Workspace>(json!({
        "id": "ws",
        "created_at": "not-a-timestamp"
    }))
    .unwrap_err();
    // `id` is a valid string, so the only possible failure is the RFC3339 parse
    // of `created_at` (chrono: "input contains invalid characters"). A missing
    // field would say "missing field", a wrong type "invalid type" — so an
    // "invalid characters" message is specific to the parse-failure cause.
    assert!(
        err.to_string().contains("invalid characters"),
        "expected created_at parse failure, got: {err}"
    );
}

#[test]
fn workspace_rejects_missing_created_at() {
    let err = serde_json::from_value::<Workspace>(json!({ "id": "ws" })).unwrap_err();
    assert!(
        err.to_string().contains("created_at"),
        "expected missing-field error for created_at, got: {err}"
    );
}

#[test]
fn workspace_create_rejects_non_string_id() {
    let err = serde_json::from_value::<WorkspaceCreate>(json!({ "id": 123 })).unwrap_err();
    assert!(
        err.to_string().contains("string") || err.to_string().contains("id"),
        "unexpected error: {err}"
    );
}

#[test]
fn workspace_search_request_rejects_missing_query() {
    let err = serde_json::from_value::<WorkspaceSearchRequest>(json!({ "limit": 10 })).unwrap_err();
    assert!(
        err.to_string().contains("query"),
        "expected missing-field error for query, got: {err}"
    );
}

#[test]
fn workspace_search_request_rejects_non_numeric_limit() {
    let err = serde_json::from_value::<WorkspaceSearchRequest>(json!({
        "query": "x",
        "limit": "not-a-number"
    }))
    .unwrap_err();
    // serde reports the type mismatch on the `u32` target field.
    assert!(
        err.to_string().contains("invalid type") || err.to_string().contains("u32"),
        "expected limit type-mismatch error, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// GATE / documented gap: WorkspaceCreate does NOT validate `id` at the type
// level. Charset (`[a-zA-Z0-9_-]+`) and length (1-512) checks live in the HTTP
// route layer (`client.rs` / `http::routes::validate_id`), so a malformed id
// round-trips through the type unchanged. This documents the gap rather than
// asserting a non-existent `Err`; if PR5/PR6 move validation into the type,
// this test should flip to `unwrap_err()`.
// ---------------------------------------------------------------------------

#[test]
fn workspace_create_does_not_validate_id_at_type_level() {
    let body = WorkspaceCreate::builder().id("bad id!").build();
    assert_eq!(body.id, "bad id!");
    let long = "a".repeat(600);
    let body = WorkspaceCreate::builder().id(long.clone()).build();
    assert_eq!(body.id, long);
}
