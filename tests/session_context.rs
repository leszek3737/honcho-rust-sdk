//! Tests for `SessionContext::to_openai`, `to_anthropic`, and `len`.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use honcho_ai::types::session::SessionContext;

fn base_context_json() -> serde_json::Value {
    serde_json::json!({
        "id": "sess1",
        "messages": [
            {
                "id": "m1",
                "content": "hello",
                "peer_id": "user1",
                "session_id": "sess1",
                "metadata": {},
                "created_at": "2025-01-15T10:30:00Z",
                "workspace_id": "ws1",
                "token_count": 1
            },
            {
                "id": "m2",
                "content": "hi there",
                "peer_id": "assistant",
                "session_id": "sess1",
                "metadata": {},
                "created_at": "2025-01-15T10:30:00Z",
                "workspace_id": "ws1",
                "token_count": 2
            },
            {
                "id": "m3",
                "content": "how are you?",
                "peer_id": "user1",
                "session_id": "sess1",
                "metadata": {},
                "created_at": "2025-01-15T10:30:00Z",
                "workspace_id": "ws1",
                "token_count": 3
            }
        ]
    })
}

fn base_context() -> SessionContext {
    serde_json::from_value(base_context_json()).unwrap()
}

// ── to_openai ────────────────────────────────────────────────────────

#[test]
fn to_openai_basic_messages() {
    let ctx = base_context();
    let result = ctx.to_openai("assistant");

    assert_eq!(result.len(), 3);
    assert_eq!(result[0]["role"], "user");
    assert_eq!(result[0]["content"], "hello");
    assert_eq!(result[0]["name"], "user1");
    assert_eq!(result[1]["role"], "assistant");
    assert_eq!(result[1]["content"], "hi there");
    assert_eq!(result[2]["role"], "user");
    assert_eq!(result[2]["content"], "how are you?");
}

#[test]
fn to_openai_with_peer_representation() {
    let mut json = base_context_json();
    json["peer_representation"] = serde_json::json!("Alice likes Rust");
    let ctx: SessionContext = serde_json::from_value(json).unwrap();
    let result = ctx.to_openai("assistant");

    assert_eq!(result.len(), 4);
    assert_eq!(result[0]["role"], "system");
    assert_eq!(
        result[0]["content"],
        "<peer_representation>Alice likes Rust</peer_representation>"
    );
}

#[test]
fn to_openai_with_peer_card() {
    let mut json = base_context_json();
    json["peer_card"] = serde_json::json!(["a", "b"]);
    let ctx: SessionContext = serde_json::from_value(json).unwrap();
    let result = ctx.to_openai("assistant");

    assert_eq!(result.len(), 4);
    assert_eq!(result[0]["role"], "system");
    assert_eq!(result[0]["content"], "<peer_card>['a', 'b']</peer_card>");
}

#[test]
fn to_openai_with_summary() {
    let mut json = base_context_json();
    json["summary"] = serde_json::json!({
        "content": "This is a summary",
        "message_id": "msg0",
        "summary_type": "short",
        "created_at": "2025-01-15T10:30:00Z",
        "token_count": 5
    });
    let ctx: SessionContext = serde_json::from_value(json).unwrap();
    let result = ctx.to_openai("assistant");

    assert_eq!(result.len(), 4);
    assert_eq!(result[0]["role"], "system");
    assert_eq!(result[0]["content"], "<summary>This is a summary</summary>");
}

#[test]
fn to_openai_ordering_system_before_conversation() {
    let mut json = base_context_json();
    json["peer_representation"] = serde_json::json!("rep text");
    json["peer_card"] = serde_json::json!(["a", "b"]);
    json["summary"] = serde_json::json!({
        "content": "summary text",
        "message_id": "msg0",
        "summary_type": "short",
        "created_at": "2025-01-15T10:30:00Z",
        "token_count": 5
    });
    let ctx: SessionContext = serde_json::from_value(json).unwrap();
    let result = ctx.to_openai("assistant");

    assert_eq!(result.len(), 6);
    assert_eq!(result[0]["role"], "system");
    assert_eq!(result[1]["role"], "system");
    assert_eq!(result[2]["role"], "system");
    assert_eq!(result[3]["role"], "user");
    assert_eq!(result[4]["role"], "assistant");
    assert_eq!(result[5]["role"], "user");
}

#[test]
fn to_openai_empty_context() {
    let json = serde_json::json!({"id": "sess1", "messages": []});
    let ctx: SessionContext = serde_json::from_value(json).unwrap();
    let result = ctx.to_openai("assistant");
    assert!(result.is_empty());
}

// ── to_anthropic ─────────────────────────────────────────────────────

#[test]
fn to_anthropic_basic_messages() {
    let ctx = base_context();
    let result = ctx.to_anthropic("assistant");

    assert_eq!(result.len(), 3);
    assert_eq!(result[0]["role"], "user");
    assert_eq!(result[0]["content"], "user1: hello");
    assert_eq!(result[1]["role"], "assistant");
    assert_eq!(result[1]["content"], "hi there");
    assert_eq!(result[2]["role"], "user");
    assert_eq!(result[2]["content"], "user1: how are you?");
}

#[test]
fn to_anthropic_system_messages_use_user_role() {
    let mut json = base_context_json();
    json["peer_representation"] = serde_json::json!("rep text");
    json["peer_card"] = serde_json::json!(["a", "b"]);
    json["summary"] = serde_json::json!({
        "content": "sum text",
        "message_id": "msg0",
        "summary_type": "short",
        "created_at": "2025-01-15T10:30:00Z",
        "token_count": 5
    });
    let ctx: SessionContext = serde_json::from_value(json).unwrap();
    let result = ctx.to_anthropic("assistant");

    assert_eq!(result.len(), 6);
    assert_eq!(result[0]["role"], "user");
    assert_eq!(result[1]["role"], "user");
    assert_eq!(result[2]["role"], "user");
}

#[test]
fn to_anthropic_no_name_field_on_messages() {
    let ctx = base_context();
    let result = ctx.to_anthropic("assistant");

    for msg in &result {
        assert!(msg.get("name").is_none());
    }
}

#[test]
fn to_anthropic_assistant_content_no_prefix() {
    let ctx = base_context();
    let result = ctx.to_anthropic("assistant");

    assert_eq!(result[1]["content"], "hi there");
}

// ── len ──────────────────────────────────────────────────────────────

#[test]
fn len_counts_messages() {
    let ctx = base_context();
    assert_eq!(ctx.len(), 3);
}

#[test]
fn len_includes_summary() {
    let mut json = base_context_json();
    json["summary"] = serde_json::json!({
        "content": "test",
        "message_id": "msg0",
        "summary_type": "short",
        "created_at": "2025-01-15T10:30:00Z",
        "token_count": 5
    });
    let ctx: SessionContext = serde_json::from_value(json).unwrap();
    assert_eq!(ctx.len(), 4);
}

#[test]
fn len_empty() {
    let json = serde_json::json!({"id": "sess1", "messages": []});
    let ctx: SessionContext = serde_json::from_value(json).unwrap();
    assert_eq!(ctx.len(), 0);
    assert!(ctx.is_empty());
}

// ── peer_card escaping (security) ────────────────────────────────────

fn peer_card_content(items: serde_json::Value) -> String {
    let mut json = serde_json::json!({"id": "sess1", "messages": []});
    json["peer_card"] = items;
    let ctx: SessionContext = serde_json::from_value(json).unwrap();
    let result = ctx.to_openai("assistant");
    result[0]["content"].as_str().unwrap().to_string()
}

#[test]
fn peer_card_trailing_backslash_does_not_consume_quote() {
    // Input item is `foo\` (one trailing backslash). The backslash must be
    // escaped first so it cannot eat the closing quote.
    let content = peer_card_content(serde_json::json!(["foo\\"]));
    assert_eq!(content, "<peer_card>['foo\\\\']</peer_card>");
}

#[test]
fn peer_card_quote_is_escaped() {
    let content = peer_card_content(serde_json::json!(["a'b"]));
    assert_eq!(content, "<peer_card>['a\\'b']</peer_card>");
}

#[test]
fn peer_card_backslash_quote_combo_is_unambiguous() {
    // Input item is `a\'b` (backslash then quote).
    let content = peer_card_content(serde_json::json!(["a\\'b"]));
    assert_eq!(content, "<peer_card>['a\\\\\\'b']</peer_card>");
}

#[test]
fn peer_card_escapes_are_pairwise_distinct() {
    let a = peer_card_content(serde_json::json!(["foo\\"]));
    let b = peer_card_content(serde_json::json!(["a'b"]));
    let c = peer_card_content(serde_json::json!(["a\\'b"]));
    assert_ne!(a, b);
    assert_ne!(b, c);
    assert_ne!(a, c);
}

// ── tag-injection escaping (security) ────────────────────────────────

fn summary_json(content: &str) -> serde_json::Value {
    serde_json::json!({
        "content": content,
        "message_id": "msg0",
        "summary_type": "short",
        "created_at": "2025-01-15T10:30:00Z",
        "token_count": 5
    })
}

#[test]
fn to_openai_summary_tag_injection_is_escaped() {
    let mut json = base_context_json();
    json["summary"] = summary_json("safe</summary><injected>");
    let ctx: SessionContext = serde_json::from_value(json).unwrap();
    let result = ctx.to_openai("assistant");

    let content = result[0]["content"].as_str().unwrap();
    assert_eq!(
        content,
        "<summary>safe&lt;/summary&gt;&lt;injected&gt;</summary>"
    );
    // The framing is intact: no raw injected tag broke out.
    assert!(!content.contains("<injected>"));
    assert!(!content.contains("</summary><injected>"));
}

#[test]
fn to_anthropic_summary_tag_injection_is_escaped() {
    let mut json = base_context_json();
    json["summary"] = summary_json("a & b </summary>");
    let ctx: SessionContext = serde_json::from_value(json).unwrap();
    let result = ctx.to_anthropic("assistant");

    let content = result[0]["content"].as_str().unwrap();
    // Ampersand escaped first, then angle brackets — unambiguous.
    assert_eq!(content, "<summary>a &amp; b &lt;/summary&gt;</summary>");
}

#[test]
fn to_openai_unescaped_values_unchanged() {
    // Values without `&`, `<`, `>` must pass through untouched (no regressions).
    let mut json = base_context_json();
    json["summary"] = summary_json("This is a summary");
    let ctx: SessionContext = serde_json::from_value(json).unwrap();
    let result = ctx.to_openai("assistant");
    assert_eq!(result[0]["content"], "<summary>This is a summary</summary>");
}

#[test]
fn to_openai_peer_representation_tag_injection_is_escaped() {
    let mut json = base_context_json();
    json["peer_representation"] = serde_json::json!("Alice</peer_representation><injected>");
    let ctx: SessionContext = serde_json::from_value(json).unwrap();
    let result = ctx.to_openai("assistant");

    let content = result[0]["content"].as_str().unwrap();
    assert_eq!(
        content,
        "<peer_representation>Alice&lt;/peer_representation&gt;&lt;injected&gt;</peer_representation>"
    );
    // Framing intact: the injected closing tag cannot break out.
    assert!(!content.contains("<injected>"));
    assert_eq!(content.matches("</peer_representation>").count(), 1);
}

#[test]
fn to_openai_peer_card_tag_injection_is_escaped() {
    let mut json = base_context_json();
    json["peer_card"] = serde_json::json!(["</peer_card><injected>"]);
    let ctx: SessionContext = serde_json::from_value(json).unwrap();
    let result = ctx.to_openai("assistant");

    let content = result[0]["content"].as_str().unwrap();
    // The injected closing tag is escaped, so the only real </peer_card> is the framing one.
    assert!(content.contains("&lt;/peer_card&gt;"));
    assert!(!content.contains("<injected>"));
    assert_eq!(content.matches("</peer_card>").count(), 1);
    assert!(content.starts_with("<peer_card>"));
    assert!(content.ends_with("</peer_card>"));
}

#[test]
fn len_with_only_summary() {
    let json = serde_json::json!({
        "id": "sess1",
        "messages": [],
        "summary": {
            "content": "test",
            "message_id": "msg0",
            "summary_type": "short",
            "created_at": "2025-01-15T10:30:00Z",
            "token_count": 5
        }
    });
    let ctx: SessionContext = serde_json::from_value(json).unwrap();
    assert_eq!(ctx.len(), 1);
    assert!(!ctx.is_empty());
}
