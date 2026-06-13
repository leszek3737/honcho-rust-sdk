use std::fmt::Write as _;

use crate::error::{HonchoError, Result};

pub(crate) const API_BASE_PATH: &str = "v3";

/// Percent-encode a path segment.
///
/// Encodes all characters except unreserved chars (`A-Za-z0-9-._~`).
/// Importantly, this encodes `/` to `%2F`, preventing segment-injection attacks
/// where a crafted ID could escape its path segment.
fn encode(s: &str) -> String {
    let mut encoded = String::with_capacity(s.len() * 3);
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(byte as char);
            }
            _ => {
                let _ = write!(encoded, "%{byte:02X}");
            }
        }
    }
    encoded
}

/// Validate that an ID is not empty, whitespace-only, or a dot-segment.
///
/// Dot-segments (`.` / `..` / `%2e` variants) are rejected because URL parsers
/// normalize them away, enabling path-traversal attacks.
fn validate_id(id: &str, name: &str) -> Result<()> {
    if id.trim().is_empty() {
        return Err(HonchoError::Validation(format!(
            "{name} must not be empty or whitespace-only"
        )));
    }
    // Reject dot-segments and their percent-encoded variants.
    // WHATWG URL parsers collapse "." and ".." (and %2e/%2E) during normalization,
    // which would let `workspace("..")` escape its path segment.
    let lower = id.to_ascii_lowercase();
    if lower == "." || lower == ".." || lower == "%2e" || lower == "%2e%2e" {
        return Err(HonchoError::Validation(format!(
            "{name} must not be a dot-segment ('.' or '..')"
        )));
    }
    Ok(())
}

/// Generate a route builder function.
///
/// Each parameter is validated via [`validate_id`] then percent-encoded via
/// [`encode`] and interpolated into the path template using named format args.
/// This centralizes the validation + encoding contract so it cannot drift
/// across the ~30 route builders.
macro_rules! route {
    (
        $(#[doc = $doc:literal])*
        $vis:vis fn $name:ident(
            $($param:ident : $param_name:literal),* $(,)?
        ) -> Result<String> {
            $template:literal
        }
    ) => {
        $(#[doc = $doc])*
        $vis fn $name($($param: &str),*) -> Result<String> {
            $(validate_id($param, $param_name)?;)*
            Ok(format!(
                $template,
                $($param = encode($param)),*
            ))
        }
    };
}

/// Builds path for listing all workspaces.
pub(crate) fn workspaces() -> String {
    format!("/{API_BASE_PATH}/workspaces")
}

/// Builds path for the workspace list endpoint.
pub(crate) fn workspaces_list() -> String {
    format!("/{API_BASE_PATH}/workspaces/list")
}

route! {
    /// Builds path for a specific workspace.
    pub(crate) fn workspace(
        workspace_id: "workspace_id",
    ) -> Result<String> {
        "/{API_BASE_PATH}/workspaces/{workspace_id}"
    }
}

route! {
    /// Builds path for workspace search.
    pub(crate) fn workspace_search(
        workspace_id: "workspace_id",
    ) -> Result<String> {
        "/{API_BASE_PATH}/workspaces/{workspace_id}/search"
    }
}

route! {
    /// Builds path for workspace queue status.
    pub(crate) fn workspace_queue_status(
        workspace_id: "workspace_id",
    ) -> Result<String> {
        "/{API_BASE_PATH}/workspaces/{workspace_id}/queue/status"
    }
}

route! {
    /// Builds path for scheduling a dream in a workspace.
    pub(crate) fn workspace_schedule_dream(
        workspace_id: "workspace_id",
    ) -> Result<String> {
        "/{API_BASE_PATH}/workspaces/{workspace_id}/schedule_dream"
    }
}

route! {
    /// Builds path for listing peers in a workspace.
    pub(crate) fn peers(
        workspace_id: "workspace_id",
    ) -> Result<String> {
        "/{API_BASE_PATH}/workspaces/{workspace_id}/peers"
    }
}

route! {
    /// Builds path for the peer list endpoint.
    pub(crate) fn peers_list(
        workspace_id: "workspace_id",
    ) -> Result<String> {
        "/{API_BASE_PATH}/workspaces/{workspace_id}/peers/list"
    }
}

route! {
    /// Builds path for a specific peer.
    pub(crate) fn peer(
        workspace_id: "workspace_id",
        peer_id: "peer_id",
    ) -> Result<String> {
        "/{API_BASE_PATH}/workspaces/{workspace_id}/peers/{peer_id}"
    }
}

route! {
    /// Builds path for peer chat.
    pub(crate) fn peer_chat(
        workspace_id: "workspace_id",
        peer_id: "peer_id",
    ) -> Result<String> {
        "/{API_BASE_PATH}/workspaces/{workspace_id}/peers/{peer_id}/chat"
    }
}

route! {
    /// Builds path for peer representation.
    pub(crate) fn peer_representation(
        workspace_id: "workspace_id",
        peer_id: "peer_id",
    ) -> Result<String> {
        "/{API_BASE_PATH}/workspaces/{workspace_id}/peers/{peer_id}/representation"
    }
}

route! {
    /// Builds path for peer card.
    pub(crate) fn peer_card(
        workspace_id: "workspace_id",
        peer_id: "peer_id",
    ) -> Result<String> {
        "/{API_BASE_PATH}/workspaces/{workspace_id}/peers/{peer_id}/card"
    }
}

route! {
    /// Builds path for peer context.
    pub(crate) fn peer_context(
        workspace_id: "workspace_id",
        peer_id: "peer_id",
    ) -> Result<String> {
        "/{API_BASE_PATH}/workspaces/{workspace_id}/peers/{peer_id}/context"
    }
}

route! {
    /// Builds path for peer search.
    pub(crate) fn peer_search(
        workspace_id: "workspace_id",
        peer_id: "peer_id",
    ) -> Result<String> {
        "/{API_BASE_PATH}/workspaces/{workspace_id}/peers/{peer_id}/search"
    }
}

route! {
    /// Builds path for listing sessions of a peer.
    pub(crate) fn peer_sessions_list(
        workspace_id: "workspace_id",
        peer_id: "peer_id",
    ) -> Result<String> {
        "/{API_BASE_PATH}/workspaces/{workspace_id}/peers/{peer_id}/sessions"
    }
}

route! {
    /// Builds path for listing sessions in a workspace.
    pub(crate) fn sessions(
        workspace_id: "workspace_id",
    ) -> Result<String> {
        "/{API_BASE_PATH}/workspaces/{workspace_id}/sessions"
    }
}

route! {
    /// Builds path for the session list endpoint.
    pub(crate) fn sessions_list(
        workspace_id: "workspace_id",
    ) -> Result<String> {
        "/{API_BASE_PATH}/workspaces/{workspace_id}/sessions/list"
    }
}

route! {
    /// Builds path for a specific session.
    pub(crate) fn session(
        workspace_id: "workspace_id",
        session_id: "session_id",
    ) -> Result<String> {
        "/{API_BASE_PATH}/workspaces/{workspace_id}/sessions/{session_id}"
    }
}

route! {
    /// Builds path for cloning a session.
    pub(crate) fn session_clone(
        workspace_id: "workspace_id",
        session_id: "session_id",
    ) -> Result<String> {
        "/{API_BASE_PATH}/workspaces/{workspace_id}/sessions/{session_id}/clone"
    }
}

route! {
    /// Builds path for session context.
    pub(crate) fn session_context(
        workspace_id: "workspace_id",
        session_id: "session_id",
    ) -> Result<String> {
        "/{API_BASE_PATH}/workspaces/{workspace_id}/sessions/{session_id}/context"
    }
}

route! {
    /// Builds path for session summaries.
    pub(crate) fn session_summaries(
        workspace_id: "workspace_id",
        session_id: "session_id",
    ) -> Result<String> {
        "/{API_BASE_PATH}/workspaces/{workspace_id}/sessions/{session_id}/summaries"
    }
}

route! {
    /// Builds path for session search.
    pub(crate) fn session_search(
        workspace_id: "workspace_id",
        session_id: "session_id",
    ) -> Result<String> {
        "/{API_BASE_PATH}/workspaces/{workspace_id}/sessions/{session_id}/search"
    }
}

route! {
    /// Builds path for listing peers in a session.
    pub(crate) fn session_peers(
        workspace_id: "workspace_id",
        session_id: "session_id",
    ) -> Result<String> {
        "/{API_BASE_PATH}/workspaces/{workspace_id}/sessions/{session_id}/peers"
    }
}

route! {
    /// Builds path for per-peer session configuration.
    pub(crate) fn session_peer_config(
        workspace_id: "workspace_id",
        session_id: "session_id",
        peer_id: "peer_id",
    ) -> Result<String> {
        "/{API_BASE_PATH}/workspaces/{workspace_id}/sessions/{session_id}/peers/{peer_id}/config"
    }
}

route! {
    /// Builds path for listing messages in a session.
    pub(crate) fn messages(
        workspace_id: "workspace_id",
        session_id: "session_id",
    ) -> Result<String> {
        "/{API_BASE_PATH}/workspaces/{workspace_id}/sessions/{session_id}/messages"
    }
}

route! {
    /// Builds path for the message list endpoint.
    pub(crate) fn messages_list(
        workspace_id: "workspace_id",
        session_id: "session_id",
    ) -> Result<String> {
        "/{API_BASE_PATH}/workspaces/{workspace_id}/sessions/{session_id}/messages/list"
    }
}

route! {
    /// Builds path for a specific message.
    pub(crate) fn message(
        workspace_id: "workspace_id",
        session_id: "session_id",
        message_id: "message_id",
    ) -> Result<String> {
        "/{API_BASE_PATH}/workspaces/{workspace_id}/sessions/{session_id}/messages/{message_id}"
    }
}

route! {
    /// Builds path for uploading a file to a session.
    pub(crate) fn messages_upload(
        workspace_id: "workspace_id",
        session_id: "session_id",
    ) -> Result<String> {
        "/{API_BASE_PATH}/workspaces/{workspace_id}/sessions/{session_id}/messages/upload"
    }
}

route! {
    /// Builds path for listing conclusions in a workspace.
    pub(crate) fn conclusions(
        workspace_id: "workspace_id",
    ) -> Result<String> {
        "/{API_BASE_PATH}/workspaces/{workspace_id}/conclusions"
    }
}

route! {
    /// Builds path for the conclusions list endpoint.
    pub(crate) fn conclusions_list(
        workspace_id: "workspace_id",
    ) -> Result<String> {
        "/{API_BASE_PATH}/workspaces/{workspace_id}/conclusions/list"
    }
}

route! {
    /// Builds path for querying conclusions.
    pub(crate) fn conclusions_query(
        workspace_id: "workspace_id",
    ) -> Result<String> {
        "/{API_BASE_PATH}/workspaces/{workspace_id}/conclusions/query"
    }
}

route! {
    /// Builds path for a specific conclusion.
    pub(crate) fn conclusion(
        workspace_id: "workspace_id",
        conclusion_id: "conclusion_id",
    ) -> Result<String> {
        "/{API_BASE_PATH}/workspaces/{workspace_id}/conclusions/{conclusion_id}"
    }
}

/// Builds path for deleting a conclusion (same path as get).
///
/// This is a semantic alias of [`conclusion`] kept for call-site readability.
#[inline]
pub(crate) fn conclusion_delete(workspace_id: &str, conclusion_id: &str) -> Result<String> {
    conclusion(workspace_id, conclusion_id)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn test_workspaces() {
        assert_eq!(workspaces(), "/v3/workspaces");
    }

    #[test]
    fn test_workspaces_list() {
        assert_eq!(workspaces_list(), "/v3/workspaces/list");
    }

    #[test]
    fn test_workspace() {
        assert_eq!(workspace("ws1").unwrap(), "/v3/workspaces/ws1");
    }

    #[test]
    fn test_workspace_search() {
        assert_eq!(
            workspace_search("ws1").unwrap(),
            "/v3/workspaces/ws1/search"
        );
    }

    #[test]
    fn test_workspace_queue_status() {
        assert_eq!(
            workspace_queue_status("ws1").unwrap(),
            "/v3/workspaces/ws1/queue/status"
        );
    }

    #[test]
    fn test_workspace_schedule_dream() {
        assert_eq!(
            workspace_schedule_dream("ws1").unwrap(),
            "/v3/workspaces/ws1/schedule_dream"
        );
    }

    #[test]
    fn test_peers() {
        assert_eq!(peers("ws1").unwrap(), "/v3/workspaces/ws1/peers");
    }

    #[test]
    fn test_peers_list() {
        assert_eq!(peers_list("ws1").unwrap(), "/v3/workspaces/ws1/peers/list");
    }

    #[test]
    fn test_peer() {
        assert_eq!(
            peer("ws1", "alice").unwrap(),
            "/v3/workspaces/ws1/peers/alice"
        );
    }

    #[test]
    fn test_peer_chat() {
        assert_eq!(
            peer_chat("ws1", "alice").unwrap(),
            "/v3/workspaces/ws1/peers/alice/chat"
        );
    }

    #[test]
    fn test_peer_representation() {
        assert_eq!(
            peer_representation("ws1", "alice").unwrap(),
            "/v3/workspaces/ws1/peers/alice/representation"
        );
    }

    #[test]
    fn test_peer_card() {
        assert_eq!(
            peer_card("ws1", "alice").unwrap(),
            "/v3/workspaces/ws1/peers/alice/card"
        );
    }

    #[test]
    fn test_peer_context() {
        assert_eq!(
            peer_context("ws1", "alice").unwrap(),
            "/v3/workspaces/ws1/peers/alice/context"
        );
    }

    #[test]
    fn test_peer_search() {
        assert_eq!(
            peer_search("ws1", "alice").unwrap(),
            "/v3/workspaces/ws1/peers/alice/search"
        );
    }

    #[test]
    fn test_peer_sessions_list() {
        assert_eq!(
            peer_sessions_list("ws1", "alice").unwrap(),
            "/v3/workspaces/ws1/peers/alice/sessions"
        );
    }

    #[test]
    fn test_sessions() {
        assert_eq!(sessions("ws1").unwrap(), "/v3/workspaces/ws1/sessions");
    }

    #[test]
    fn test_sessions_list() {
        assert_eq!(
            sessions_list("ws1").unwrap(),
            "/v3/workspaces/ws1/sessions/list"
        );
    }

    #[test]
    fn test_session() {
        assert_eq!(
            session("ws1", "sess1").unwrap(),
            "/v3/workspaces/ws1/sessions/sess1"
        );
    }

    #[test]
    fn test_session_clone() {
        assert_eq!(
            session_clone("ws1", "sess1").unwrap(),
            "/v3/workspaces/ws1/sessions/sess1/clone"
        );
    }

    #[test]
    fn test_session_context() {
        assert_eq!(
            session_context("ws1", "sess1").unwrap(),
            "/v3/workspaces/ws1/sessions/sess1/context"
        );
    }

    #[test]
    fn test_session_summaries() {
        assert_eq!(
            session_summaries("ws1", "sess1").unwrap(),
            "/v3/workspaces/ws1/sessions/sess1/summaries"
        );
    }

    #[test]
    fn test_session_search() {
        assert_eq!(
            session_search("ws1", "sess1").unwrap(),
            "/v3/workspaces/ws1/sessions/sess1/search"
        );
    }

    #[test]
    fn test_session_peers() {
        assert_eq!(
            session_peers("ws1", "sess1").unwrap(),
            "/v3/workspaces/ws1/sessions/sess1/peers"
        );
    }

    #[test]
    fn test_session_peer_config() {
        assert_eq!(
            session_peer_config("ws1", "sess1", "alice").unwrap(),
            "/v3/workspaces/ws1/sessions/sess1/peers/alice/config"
        );
    }

    #[test]
    fn test_messages() {
        assert_eq!(
            messages("ws1", "sess1").unwrap(),
            "/v3/workspaces/ws1/sessions/sess1/messages"
        );
    }

    #[test]
    fn test_messages_list() {
        assert_eq!(
            messages_list("ws1", "sess1").unwrap(),
            "/v3/workspaces/ws1/sessions/sess1/messages/list"
        );
    }

    #[test]
    fn test_message() {
        assert_eq!(
            message("ws1", "sess1", "msg1").unwrap(),
            "/v3/workspaces/ws1/sessions/sess1/messages/msg1"
        );
    }

    #[test]
    fn test_messages_upload() {
        assert_eq!(
            messages_upload("ws1", "sess1").unwrap(),
            "/v3/workspaces/ws1/sessions/sess1/messages/upload"
        );
    }

    #[test]
    fn test_conclusions() {
        assert_eq!(
            conclusions("ws1").unwrap(),
            "/v3/workspaces/ws1/conclusions"
        );
    }

    #[test]
    fn test_conclusions_list() {
        assert_eq!(
            conclusions_list("ws1").unwrap(),
            "/v3/workspaces/ws1/conclusions/list"
        );
    }

    #[test]
    fn test_conclusions_query() {
        assert_eq!(
            conclusions_query("ws1").unwrap(),
            "/v3/workspaces/ws1/conclusions/query"
        );
    }

    #[test]
    fn test_conclusion() {
        assert_eq!(
            conclusion("ws1", "conc1").unwrap(),
            "/v3/workspaces/ws1/conclusions/conc1"
        );
    }

    #[test]
    fn test_workspace_empty_id_returns_validation_error() {
        let err = workspace("").unwrap_err();
        assert!(matches!(err, HonchoError::Validation(_)));
        assert!(format!("{err}").contains("workspace_id"));
    }

    #[test]
    fn test_peer_empty_peer_id_returns_validation_error() {
        let err = peer("ws1", "").unwrap_err();
        assert!(matches!(err, HonchoError::Validation(_)));
        assert!(format!("{err}").contains("peer_id"));
    }

    #[test]
    fn test_session_empty_session_id_returns_validation_error() {
        let err = session("ws1", "").unwrap_err();
        assert!(matches!(err, HonchoError::Validation(_)));
        assert!(format!("{err}").contains("session_id"));
    }

    #[test]
    fn test_message_empty_message_id_returns_validation_error() {
        let err = message("ws1", "sess1", "").unwrap_err();
        assert!(matches!(err, HonchoError::Validation(_)));
        assert!(format!("{err}").contains("message_id"));
    }

    #[test]
    fn test_conclusion_empty_id_returns_validation_error() {
        let err = conclusion("ws1", "").unwrap_err();
        assert!(matches!(err, HonchoError::Validation(_)));
        assert!(format!("{err}").contains("conclusion_id"));
    }

    #[test]
    fn test_workspace_dot_segment_rejected() {
        assert!(matches!(
            workspace(".").unwrap_err(),
            HonchoError::Validation(_)
        ));
        assert!(matches!(
            workspace("..").unwrap_err(),
            HonchoError::Validation(_)
        ));
    }

    #[test]
    fn test_workspace_percent_encoded_dot_rejected() {
        assert!(matches!(
            workspace("%2e").unwrap_err(),
            HonchoError::Validation(_)
        ));
        assert!(matches!(
            workspace("%2e%2e").unwrap_err(),
            HonchoError::Validation(_)
        ));
        assert!(matches!(
            workspace("%2E").unwrap_err(),
            HonchoError::Validation(_)
        ));
    }

    #[test]
    fn test_workspace_whitespace_only_rejected() {
        assert!(matches!(
            workspace("   ").unwrap_err(),
            HonchoError::Validation(_)
        ));
    }

    #[test]
    fn test_encode_space() {
        assert_eq!(encode("hello world"), "hello%20world");
    }

    #[test]
    fn test_encode_slash() {
        assert_eq!(encode("a/b"), "a%2Fb");
    }

    #[test]
    fn test_encode_unicode() {
        assert_eq!(encode("café"), "caf%C3%A9");
    }

    #[test]
    fn test_encode_percent() {
        assert_eq!(encode("100%"), "100%25");
    }

    #[test]
    fn test_encode_unreserved() {
        assert_eq!(encode("A-Z.0-9_-~"), "A-Z.0-9_-~");
    }
}
