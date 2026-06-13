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
