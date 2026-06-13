#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

use honcho_ai::error::HonchoError;
use honcho_ai::http::decode::deserialize_with_path;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct RequiresString {
    #[allow(dead_code)] // Field intentionally unused — tests decoder path
    id: String,
}

#[test]
fn decode_malformed_json_returns_decode_with_path() {
    let json = r#"{"id":null}"#;
    let result: Result<RequiresString, _> = deserialize_with_path(json.as_bytes());

    match result {
        Err(HonchoError::Decode { path, .. }) => {
            assert!(
                path.contains("id") || path.contains("root"),
                "path should contain field name, got: {path}"
            );
        }
        Err(other) => panic!("expected Decode error, got {other:?}"),
        Ok(_) => panic!("expected error, got success"),
    }
}

#[test]
fn decode_trailing_data_rejected() {
    let input = b"{\"id\":\"x\"}garbage";
    let result: Result<RequiresString, _> = deserialize_with_path(input);
    assert!(
        matches!(result, Err(HonchoError::Decode { .. })),
        "trailing data should be rejected with Decode error, got {result:?}"
    );
}

#[test]
fn decode_clean_json_succeeds() {
    let input = b"{\"id\":\"x\"}";
    let result: RequiresString = deserialize_with_path(input).expect("clean JSON should decode");
    assert_eq!(result.id, "x");
}
