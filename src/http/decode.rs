//! Response deserialization with path tracking.

use serde::de::DeserializeOwned;

use crate::error::{HonchoError, Result};

/// Deserialize bytes into `T` with `serde_path_to_error` tracking.
///
/// Unlike `serde_json::from_slice`, this uses `serde_path_to_error` to report
/// the exact JSON path where a decode error occurred. A root-level error
/// yields `path = "."`.
///
/// Trailing data after the first JSON value is rejected (matching
/// `serde_json::from_slice` behavior) to surface corrupted or concatenated
/// responses rather than silently ignoring them.
///
/// # Errors
/// Returns `HonchoError::Decode` if deserialization fails or if trailing
/// data is present after the first JSON value.
pub fn deserialize_with_path<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    let mut de = serde_json::Deserializer::from_slice(bytes);
    let value = serde_path_to_error::deserialize(&mut de).map_err(|err| HonchoError::Decode {
        path: err.path().to_string(),
        source: err.into_inner(),
    })?;
    de.end().map_err(|err| HonchoError::Decode {
        path: ".".to_string(),
        source: err,
    })?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    //! Tests for `deserialize_with_path` — response decoding with
    //! `serde_path_to_error` path tracking and `HonchoError::Decode`
    //! source-chain wiring. Recovered from the former `tests/decode_error.rs`,
    //! ported inline now that `http` is `pub(crate)`.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::error::Error as _;

    use rstest::rstest;
    use serde::Deserialize;
    use serde_json::error::Category;

    use super::deserialize_with_path;
    use crate::error::HonchoError;

    #[derive(Debug, Deserialize)]
    struct RequiresString {
        #[allow(dead_code)] // Field intentionally unused — tests decoder path
        id: String,
    }

    #[derive(Debug, Deserialize)]
    struct Inner {
        #[allow(dead_code)] // Field intentionally unused — tests decoder path
        inner: String,
    }

    #[derive(Debug, Deserialize)]
    struct Nested {
        #[allow(dead_code)] // Field intentionally unused — tests decoder path
        outer: Inner,
    }

    #[derive(Debug, Deserialize)]
    struct WithArray {
        #[allow(dead_code)] // Field intentionally unused — tests decoder path
        arr: Vec<String>,
    }

    /// Assert the result is `Err(HonchoError::Decode { path, source })` and
    /// return references to both. Verifies that the `#[source]` attribute is
    /// reachable via the `std::error::Error::source` chain (not only via field
    /// destructuring).
    fn expect_decode_err<T>(result: &Result<T, HonchoError>) -> (&str, &serde_json::Error)
    where
        T: std::fmt::Debug,
    {
        let Err(err) = result else {
            panic!("expected Err, got Ok");
        };
        let HonchoError::Decode { path, source } = err else {
            panic!("expected Decode variant, got {err:?}");
        };
        let via_chain = err
            .source()
            .and_then(|s| s.downcast_ref::<serde_json::Error>())
            .expect("source chain must expose the serde_json::Error");
        assert!(
            std::ptr::eq(via_chain, source),
            "source chain must point at the same serde_json::Error as the field"
        );
        (path.as_str(), source)
    }

    #[test]
    fn decode_valid_json_returns_ok() {
        let input = br#"{"id":"x"}"#;
        let result: RequiresString =
            deserialize_with_path(input).expect("valid JSON should decode");
        assert_eq!(result.id, "x");
    }

    #[test]
    fn decode_type_mismatch_at_flat_field_reports_exact_path() {
        // `{"id":null}` is VALID JSON — a type mismatch, not malformed JSON.
        let input = br#"{"id":null}"#;
        let result = deserialize_with_path::<RequiresString>(input);
        let (path, source) = expect_decode_err(&result);
        assert_eq!(path, "id");
        assert_eq!(
            source.classify(),
            Category::Data,
            "underlying error: {source}"
        );
    }

    #[test]
    fn decode_malformed_json_within_field_reports_field_path() {
        // Genuinely malformed JSON (invalid literal `tru}` after the colon):
        // even syntax errors are path-tracked to the field being parsed.
        let input = br#"{"id": tru}"#;
        let result = deserialize_with_path::<RequiresString>(input);
        let (path, source) = expect_decode_err(&result);
        assert_eq!(path, "id");
        assert_eq!(
            source.classify(),
            Category::Syntax,
            "underlying error: {source}"
        );
    }

    #[test]
    fn decode_type_mismatch_at_nested_field_reports_dotted_path() {
        let input = br#"{"outer":{"inner":null}}"#;
        let result = deserialize_with_path::<Nested>(input);
        let (path, source) = expect_decode_err(&result);
        assert_eq!(path, "outer.inner");
        assert_eq!(
            source.classify(),
            Category::Data,
            "underlying error: {source}"
        );
    }

    #[test]
    fn decode_type_mismatch_at_array_index_reports_indexed_path() {
        let input = br#"{"arr":[123]}"#;
        let result = deserialize_with_path::<WithArray>(input);
        let (path, source) = expect_decode_err(&result);
        assert_eq!(path, "arr[0]");
        assert_eq!(
            source.classify(),
            Category::Data,
            "underlying error: {source}"
        );
    }

    #[test]
    fn decode_trailing_data_rejected_at_root() {
        let input = br#"{"id":"x"}garbage"#;
        let result = deserialize_with_path::<RequiresString>(input);
        let (path, source) = expect_decode_err(&result);
        assert_eq!(path, ".");
        assert_eq!(
            source.classify(),
            Category::Syntax,
            "underlying error: {source}"
        );
        assert!(
            !source.to_string().is_empty(),
            "underlying error message should be non-empty: {source}"
        );
    }

    #[rstest]
    #[case::empty(b"".as_slice(), Category::Eof)]
    #[case::syntax(b"x", Category::Syntax)]
    #[case::missing_field(b"{}", Category::Data)]
    fn decode_root_level_errors_report_dot_path_and_category(
        #[case] input: &[u8],
        #[case] expected_category: Category,
    ) {
        let result = deserialize_with_path::<RequiresString>(input);
        let (path, source) = expect_decode_err(&result);
        assert_eq!(path, ".");
        assert_eq!(
            source.classify(),
            expected_category,
            "input={input:?}, underlying error: {source}"
        );
    }
}
