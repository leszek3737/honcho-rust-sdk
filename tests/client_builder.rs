#![allow(clippy::unwrap_used, clippy::expect_used)]

use honcho_ai::client::{Environment, Honcho};

fn build_honcho(params: honcho_ai::client::HonchoParams) -> Honcho {
    Honcho::from_params(params).unwrap()
}

#[test]
fn builder_with_explicit_api_key_succeeds() {
    let honcho = build_honcho(
        Honcho::builder()
            .base_url("http://localhost:8000")
            .api_key("test-key-abc123")
            .build(),
    );
    assert_eq!(honcho.workspace_id(), "default");
}

#[test]
fn builder_without_api_key_anywhere_builds_ok_without_auth() {
    let honcho = build_honcho(Honcho::builder().base_url("http://localhost:8000").build());
    assert_eq!(honcho.workspace_id(), "default");
}

#[test]
fn builder_environment_local() {
    let honcho = build_honcho(Honcho::builder().environment(Environment::Local).build());
    assert_eq!(
        honcho.base_url(),
        &url::Url::parse("http://localhost:8000").unwrap()
    );
}

#[test]
fn builder_workspace_id_default() {
    let honcho = build_honcho(Honcho::builder().base_url("http://localhost:8000").build());
    assert_eq!(honcho.workspace_id(), "default");
}

#[test]
fn builder_workspace_id_explicit() {
    let honcho = build_honcho(
        Honcho::builder()
            .base_url("http://localhost:8000")
            .workspace_id("my-workspace")
            .build(),
    );
    assert_eq!(honcho.workspace_id(), "my-workspace");
}

#[test]
fn builder_accepts_valid_workspace_id() {
    let honcho = build_honcho(
        Honcho::builder()
            .base_url("http://localhost:8000")
            .workspace_id("abc-XYZ_123")
            .build(),
    );
    assert_eq!(honcho.workspace_id(), "abc-XYZ_123");
}

#[test]
fn builder_rejects_invalid_workspace_ids() {
    for workspace_id in ["", "has space", "slash/id", "nonascii-é"] {
        let result = Honcho::from_params(
            Honcho::builder()
                .base_url("http://localhost:8000")
                .workspace_id(workspace_id)
                .build(),
        );
        assert!(result.is_err(), "accepted workspace_id: {workspace_id}");
    }
}

#[test]
fn builder_rejects_too_long_workspace_id() {
    let result = Honcho::from_params(
        Honcho::builder()
            .base_url("http://localhost:8000")
            .workspace_id("a".repeat(513))
            .build(),
    );
    assert!(result.is_err());
}

#[test]
fn builder_rejects_invalid_base_urls() {
    for base_url in ["localhost:8000", "ftp://example.com", "http://"] {
        let result = Honcho::from_params(Honcho::builder().base_url(base_url).build());
        assert!(result.is_err(), "accepted base_url: {base_url}");
    }
}

#[test]
fn builder_normalizes_subpath_trailing_slash() {
    let honcho = build_honcho(
        Honcho::builder()
            .base_url("http://localhost:8000/api/")
            .build(),
    );
    assert_eq!(honcho.base_url().as_str(), "http://localhost:8000/api");
}

#[test]
fn builder_workspace_id_from_env_then_arg_overrides() {
    temp_env::with_var("HONCHO_WORKSPACE_ID", Some("env-workspace"), || {
        let honcho = build_honcho(Honcho::builder().workspace_id("arg-workspace").build());
        assert_eq!(honcho.workspace_id(), "arg-workspace");
    });
}
