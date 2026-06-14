#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

use std::time::Duration;

use honcho_ai::error::Result;
use honcho_ai::{FileSource, HonchoError, Message, Session, UploadFileBuilder};
use serde_json::json;

use crate::common::{WorkspaceGuard, try_client};

/// Hard ceiling for a single upload round-trip. Streaming has its own 30s budget
/// elsewhere; mirror it here so a stuck upload fails the test instead of hanging
/// the whole suite forever.
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(30);

/// Spin up an isolated workspace, register `peer_id`, create `session_id`, and
/// add the peer to it.
///
/// Returns the shared RAII [`WorkspaceGuard`] (teardown runs on drop, even on a
/// panic/unwind, so a failing assertion can never leak the workspace) together
/// with the ready-to-use session. `None` means no reachable server — the caller
/// skips, exactly like the rest of the integration suite.
async fn setup_session(peer_id: &str, session_id: &str) -> Option<(WorkspaceGuard, Session)> {
    let guard = WorkspaceGuard::new(try_client().await?);
    // Each `guard.client()` borrow is confined to its own statement; the produced
    // `Peer`/`Session` are owned (Arc-backed), so the guard can still be moved out.
    guard.client().peer(peer_id).build().await.unwrap();
    let session = guard.client().session(session_id).build().await.unwrap();
    session.add_peer(peer_id).await.unwrap();
    Some((guard, session))
}

/// Drive an upload `send()` under [`UPLOAD_TIMEOUT`]. A timeout is *always* a test
/// failure (the upload got stuck), so it panics; the upload's own `Result` is
/// returned verbatim so callers can decide whether an `Err` is expected.
async fn send_under_timeout(builder: UploadFileBuilder<'_>, ctx: &str) -> Result<Vec<Message>> {
    tokio::time::timeout(UPLOAD_TIMEOUT, builder.send())
        .await
        .unwrap_or_else(|_| {
            panic!(
                "{ctx}: upload did not complete within {}s",
                UPLOAD_TIMEOUT.as_secs()
            )
        })
}

/// Send an upload that is expected to succeed.
///
/// `try_client` already proved connectivity via `force_ensure`, so once we are
/// here any `Err` is a genuine server/protocol fault (4xx/5xx, malformed
/// multipart, ...) and MUST fail the test loudly rather than ship green.
async fn upload_ok(builder: UploadFileBuilder<'_>, ctx: &str) -> Vec<Message> {
    send_under_timeout(builder, ctx)
        .await
        .unwrap_or_else(|e| panic!("{ctx}: upload failed after a successful connect: {e}"))
}

/// Assert full upload fidelity: at least one message, every message attributed to
/// `expected_peer`, and the concatenation of all message contents equal to the
/// uploaded text (robust to server-side chunking).
///
/// Note: a [`Message`] carries no MIME field, so content fidelity of a
/// `text/plain` payload is the strongest available proxy that the server
/// accepted and parsed the declared content type. Outright MIME *rejection* is
/// covered by [`upload_rejects_malformed_mime`].
fn assert_text_upload(messages: &[Message], expected_text: &str, expected_peer: &str) {
    assert!(!messages.is_empty(), "upload returned zero messages");
    assert!(
        messages.iter().all(|m| m.peer_id() == expected_peer),
        "every uploaded message must be attributed to `{expected_peer}`"
    );
    let combined: String = messages.iter().map(Message::content).collect();
    assert_eq!(
        combined, expected_text,
        "uploaded content must round-trip verbatim"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_bytes_file_to_session() {
    let Some((_guard, session)) = setup_session("upload-bytes-peer", "upload-bytes-session").await
    else {
        return;
    };

    let data = vec![b'X'; 1024];
    let expected = std::str::from_utf8(&data).unwrap().to_owned();

    let messages = upload_ok(
        session
            .upload_file(FileSource::bytes("test.txt", data, "text/plain"))
            .peer("upload-bytes-peer"),
        "upload_bytes",
    )
    .await;

    // A 1 KiB plaintext payload is a single chunk: exact count kills the old
    // `!is_empty()` false-positive.
    assert_eq!(messages.len(), 1, "1 KiB plaintext must yield one message");
    assert_text_upload(&messages, &expected, "upload-bytes-peer");
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_streamed_file_to_session() {
    let Some((_guard, session)) =
        setup_session("upload-stream-peer", "upload-stream-session").await
    else {
        return;
    };

    let data = vec![b'A'; 512];
    let expected = std::str::from_utf8(&data).unwrap().to_owned();
    let cursor = std::io::Cursor::new(data);

    let messages = upload_ok(
        session
            .upload_file_streamed("streamed.txt", cursor, "text/plain")
            .peer("upload-stream-peer"),
        "upload_streamed",
    )
    .await;

    assert_eq!(messages.len(), 1, "512 B plaintext must yield one message");
    assert_text_upload(&messages, &expected, "upload-stream-peer");
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_path_file_to_session() {
    let Some((_guard, session)) = setup_session("upload-path-peer", "upload-path-session").await
    else {
        return;
    };

    // `FileSource::Path` streams straight from disk and was previously untested in
    // integration. Use a `.txt` suffix so reqwest sniffs `text/plain`.
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("notes.txt");
    let expected = "honcho integration path upload payload";
    std::fs::write(&file_path, expected).unwrap();

    let messages = upload_ok(
        session
            .upload_file(FileSource::path(&file_path))
            .peer("upload-path-peer"),
        "upload_path",
    )
    .await;

    assert_eq!(
        messages.len(),
        1,
        "small plaintext file must yield one message"
    );
    assert_text_upload(&messages, expected, "upload-path-peer");
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_large_file_to_session() {
    // 64 KiB of a single repeated byte: large enough to exercise (potential)
    // server-side chunking, while remaining trivially verifiable.
    const SIZE: usize = 64 * 1024;

    let Some((_guard, session)) = setup_session("upload-large-peer", "upload-large-session").await
    else {
        return;
    };

    let data = vec![b'L'; SIZE];

    let messages = upload_ok(
        session
            .upload_file(FileSource::bytes("large.txt", data, "text/plain"))
            .peer("upload-large-peer"),
        "upload_large",
    )
    .await;

    assert!(!messages.is_empty(), "large upload returned zero messages");
    assert!(
        messages.iter().all(|m| m.peer_id() == "upload-large-peer"),
        "every chunk must be attributed to the uploading peer"
    );
    assert!(
        messages
            .iter()
            .all(|m| m.content().bytes().all(|b| b == b'L')),
        "chunk content must be pure payload bytes, no corruption"
    );
    let total: usize = messages.iter().map(|m| m.content().len()).sum();
    assert_eq!(
        total, SIZE,
        "reassembled payload length must match the source"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_with_builder_options() {
    let Some((_guard, session)) = setup_session("upload-opts-peer", "upload-opts-session").await
    else {
        return;
    };

    let expected = "payload with builder options";
    let created_at = chrono::Utc::now();

    let messages = upload_ok(
        session
            .upload_file(FileSource::bytes("opts.txt", expected, "text/plain"))
            .peer("upload-opts-peer")
            .metadata(json!({ "source": "integration-upload" }))
            .created_at(created_at),
        "upload_with_options",
    )
    .await;

    assert_text_upload(&messages, expected, "upload-opts-peer");
    assert!(
        messages
            .iter()
            .all(|m| m.metadata().get("source") == Some(&json!("integration-upload"))),
        "builder `.metadata(..)` must propagate to every created message"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_attributes_distinct_peers() {
    let Some(guard) = try_client().await.map(WorkspaceGuard::new) else {
        return;
    };
    let session = guard
        .client()
        .session("upload-multi-session")
        .build()
        .await
        .unwrap();
    for peer in ["upload-multi-alice", "upload-multi-bob"] {
        guard.client().peer(peer).build().await.unwrap();
        session.add_peer(peer).await.unwrap();
    }

    let alice = upload_ok(
        session
            .upload_file(FileSource::bytes("alice.txt", "from alice", "text/plain"))
            .peer("upload-multi-alice"),
        "upload_multi_alice",
    )
    .await;
    assert_text_upload(&alice, "from alice", "upload-multi-alice");

    let bob = upload_ok(
        session
            .upload_file(FileSource::bytes("bob.txt", "from bob", "text/plain"))
            .peer("upload-multi-bob"),
        "upload_multi_bob",
    )
    .await;
    assert_text_upload(&bob, "from bob", "upload-multi-bob");
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_rejects_malformed_mime() {
    let Some((_guard, session)) = setup_session("upload-mime-peer", "upload-mime-session").await
    else {
        return;
    };

    // A control character makes the content type an invalid HTTP header value;
    // the client rejects it up front (before any request) as a `Validation`
    // error. This is deterministic regardless of server-side MIME policy.
    let err = send_under_timeout(
        session
            .upload_file(FileSource::bytes("bad.txt", "data", "text/plain\n"))
            .peer("upload-mime-peer"),
        "upload_bad_mime",
    )
    .await
    .expect_err("a malformed content type must be rejected");

    assert!(
        matches!(err, HonchoError::Validation(_)),
        "malformed MIME must surface as Validation, got: {err:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_for_peer_outside_session() {
    let Some(guard) = try_client().await.map(WorkspaceGuard::new) else {
        return;
    };
    // `insider` belongs to the session; `outsider` exists in the workspace but is
    // never added to it.
    guard.client().peer("upload-insider").build().await.unwrap();
    guard
        .client()
        .peer("upload-outsider")
        .build()
        .await
        .unwrap();
    let session = guard
        .client()
        .session("upload-outsider-session")
        .build()
        .await
        .unwrap();
    session.add_peer("upload-insider").await.unwrap();

    let result = send_under_timeout(
        session
            .upload_file(FileSource::bytes("x.txt", "outsider payload", "text/plain"))
            .peer("upload-outsider"),
        "upload_peer_outside_session",
    )
    .await;

    // The server owns this policy: it may implicitly associate the peer (then
    // every message is attributed to it) or reject the request. Either is fine;
    // a transport/timeout/decode fault is NOT — it would mean a real defect.
    match result {
        Ok(messages) => {
            assert!(
                messages.iter().all(|m| m.peer_id() == "upload-outsider"),
                "implicitly-associated upload must attribute to the named peer"
            );
        }
        Err(e) => assert!(
            matches!(
                e,
                HonchoError::BadRequest { .. }
                    | HonchoError::NotFound { .. }
                    | HonchoError::Conflict { .. }
                    | HonchoError::UnprocessableEntity { .. }
                    | HonchoError::Client { .. }
                    | HonchoError::Validation(_)
            ),
            "cross-session peer rejection must be an API-level error, got: {e:?}"
        ),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_empty_file() {
    let Some((_guard, session)) = setup_session("upload-empty-peer", "upload-empty-session").await
    else {
        return;
    };

    let result = send_under_timeout(
        session
            .upload_file(FileSource::bytes(
                "empty.txt",
                Vec::<u8>::new(),
                "text/plain",
            ))
            .peer("upload-empty-peer"),
        "upload_empty",
    )
    .await;

    // An empty file is a genuine edge case: the server may produce zero messages
    // or reject it. Both are acceptable; a transport-level failure is not.
    match result {
        Ok(messages) => assert!(
            messages.iter().all(|m| m.peer_id() == "upload-empty-peer"),
            "any message from an empty upload must still be attributed correctly"
        ),
        Err(e) => assert!(
            matches!(
                e,
                HonchoError::BadRequest { .. }
                    | HonchoError::UnprocessableEntity { .. }
                    | HonchoError::Client { .. }
                    | HonchoError::Validation(_)
            ),
            "empty-file rejection must be an API-level error, got: {e:?}"
        ),
    }
}
