//! Dream API types — background memory consolidation scheduling.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Types of dreams that can be triggered.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DreamType {
    /// Omni dream — consolidate all observations.
    #[serde(rename = "omni")]
    Omni,
    /// Unknown dream type — forward-compatibility catch-all for unrecognised variants.
    #[serde(other, rename = "unknown")]
    Unknown,
}

/// Request to schedule a dream task.
///
/// Maps `ScheduleDreamRequest` from the `OpenAPI` spec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
#[non_exhaustive]
#[builder(derive(Debug), on(String, into))]
#[builder(finish_fn = build)]
pub struct ScheduleDreamRequest {
    /// Observer peer name.
    pub observer: String,
    /// Type of dream to schedule.
    #[builder(default = DreamType::Omni)]
    pub dream_type: DreamType,
    /// Observed peer name (defaults to observer if not specified).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed: Option<String>,
    /// Session ID to scope the dream to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// Status for a specific session within the processing queue.
///
/// Maps `SessionQueueStatus` from the `OpenAPI` spec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SessionQueueStatus {
    /// Session ID if filtered by session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Total work units.
    pub total_work_units: u64,
    /// Completed work units (since last periodic cleanup).
    pub completed_work_units: u64,
    /// Work units currently being processed.
    pub in_progress_work_units: u64,
    /// Work units waiting to be processed.
    pub pending_work_units: u64,
}

/// Aggregated processing queue status.
///
/// Tracks user-facing task types only: representation, summary, and dream.
/// Internal infrastructure tasks (reconciler, webhook, deletion) are excluded.
///
/// Maps `QueueStatus` from the `OpenAPI` spec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct QueueStatus {
    /// Total work units.
    pub total_work_units: u64,
    /// Completed work units (since last periodic cleanup).
    pub completed_work_units: u64,
    /// Work units currently being processed.
    pub in_progress_work_units: u64,
    /// Work units waiting to be processed.
    pub pending_work_units: u64,
    /// Per-session status when not filtered by session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sessions: Option<HashMap<String, SessionQueueStatus>>,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]
mod tests {
    use std::collections::HashSet;

    use serde_json::json;

    use super::*;

    #[test]
    fn dream_type_unknown_catches_future_variants() {
        // Any unrecognised wire string must deserialise to Unknown, not Err.
        let got: DreamType = serde_json::from_value(json!("some_future_dream")).unwrap();
        assert_eq!(got, DreamType::Unknown);
    }

    #[test]
    fn dream_type_omni_roundtrips_exact_wire_string() {
        // Must still produce "omni" — unchanged from the previous rename_all = "lowercase" encoding.
        assert_eq!(serde_json::to_string(&DreamType::Omni).unwrap(), "\"omni\"");
        let got: DreamType = serde_json::from_value(json!("omni")).unwrap();
        assert_eq!(got, DreamType::Omni);
    }

    #[test]
    fn dream_type_is_hashable() {
        // Validates the new Hash derive; duplicate insertion must be deduplicated.
        let mut set = HashSet::new();
        set.insert(DreamType::Omni);
        set.insert(DreamType::Unknown);
        set.insert(DreamType::Omni); // duplicate — should not grow the set
        assert_eq!(set.len(), 2);
    }
}
