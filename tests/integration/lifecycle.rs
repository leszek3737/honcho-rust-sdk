#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::redundant_closure_for_method_calls,
    clippy::print_stderr,
    missing_docs
)]

use std::collections::HashMap;
use std::collections::HashSet;
use std::time::Duration;

use honcho_ai::ConclusionCreateParams;
use honcho_ai::error::HonchoError;
use honcho_ai::session::PeerSpec;
use honcho_ai::types::session::{SessionConfiguration, SessionPeerConfig};
use serde_json::json;

use crate::common::{WorkspaceGuard, try_client};

/// Spins up an isolated workspace client wrapped in the shared, awaited
/// [`WorkspaceGuard`], or returns `None` when no server is reachable (the
/// caller then self-skips).
///
/// The guard's `Drop` deletes the whole workspace via a blocking
/// `block_in_place` + `block_on`, so every test that uses it MUST run on a
/// `#[tokio::test(flavor = "multi_thread")]` runtime.
async fn guarded_client() -> Option<WorkspaceGuard> {
    Some(WorkspaceGuard::new(try_client().await?))
}

#[tokio::test(flavor = "multi_thread")]
async fn full_lifecycle() {
    let Some(guard) = guarded_client().await else {
        return;
    };
    let client = guard.client();

    let peer_a = client.peer("lifecycle-alice").build().await.unwrap();
    assert_eq!(peer_a.id(), "lifecycle-alice");

    let peer_b = client.peer("lifecycle-bob").build().await.unwrap();
    assert_eq!(peer_b.id(), "lifecycle-bob");

    let session = client.session("lifecycle-session").build().await.unwrap();
    assert_eq!(session.id(), "lifecycle-session");
    assert!(session.is_active());

    session
        .add_peers([PeerSpec::Id("lifecycle-alice".to_owned())])
        .await
        .unwrap();
    session
        .add_peers([PeerSpec::Id("lifecycle-bob".to_owned())])
        .await
        .unwrap();

    let peers = session.peers().await.unwrap();
    let peer_id_set: HashSet<&str> = peers.iter().map(honcho_ai::Peer::id).collect();
    assert_eq!(
        peer_id_set,
        HashSet::from(["lifecycle-alice", "lifecycle-bob"])
    );

    let msg_a = peer_a.message("Hello from Alice").build().unwrap();
    let msg_b = peer_b.message("Hello from Bob").build().unwrap();
    let created = session.add_messages(vec![msg_a, msg_b]).await.unwrap();
    assert_eq!(created.len(), 2);

    // Listing returns exactly the two messages, attributed to their authors.
    let listed = session.messages().await.unwrap().items();
    assert_eq!(
        listed.len(),
        2,
        "expected exactly the two messages just added"
    );
    let author_ids: HashSet<&str> = listed.iter().map(honcho_ai::Message::peer_id).collect();
    assert_eq!(
        author_ids,
        HashSet::from(["lifecycle-alice", "lifecycle-bob"])
    );

    // Context echoes the session id and carries the messages we added.
    let ctx = session.context().await.unwrap();
    assert_eq!(ctx.id, session.id());
    assert!(
        !ctx.messages.is_empty(),
        "session context should include the session messages"
    );

    // Search is eventually consistent: retry until the indexed messages surface.
    let mut search_results = Vec::new();
    let mut delay = Duration::from_millis(500);
    let max_attempts = 5;
    for attempt in 0..max_attempts {
        match session.search("Hello").await {
            Ok(r) if !r.is_empty() => {
                search_results = r;
                break;
            }
            // Not indexed yet, or a transient 5xx: back off and retry.
            Ok(_) | Err(HonchoError::Server { .. }) => {}
            Err(e) => panic!("search('Hello') failed: {e}"),
        }
        if attempt + 1 < max_attempts {
            tokio::time::sleep(delay).await;
            delay *= 2;
        }
    }
    assert!(
        !search_results.is_empty(),
        "search('Hello') returned no results after {max_attempts} attempts"
    );

    let mut meta = HashMap::new();
    meta.insert("updated".to_owned(), json!(true));
    peer_a.set_metadata(meta).await.unwrap();
    let refreshed = peer_a.get_metadata().await.unwrap();
    assert_eq!(refreshed.get("updated").unwrap(), &json!(true));

    // Zero-alloc membership checks over the raw page slices.
    let peers_page = client.peers().await.unwrap();
    assert!(
        peers_page
            .items_ref()
            .iter()
            .any(|p| p.id == "lifecycle-alice")
    );
    assert!(
        peers_page
            .items_ref()
            .iter()
            .any(|p| p.id == "lifecycle-bob")
    );

    let sessions_page = client.sessions().await.unwrap();
    assert!(
        sessions_page
            .items_ref()
            .iter()
            .any(|s| s.id == "lifecycle-session")
    );

    let fetched = session.get_message(created[0].id()).await.unwrap();
    assert_eq!(fetched.id(), created[0].id());

    // Error path: fetching a well-formed but unknown id must surface an error.
    let missing = session
        .get_message("00000000-0000-0000-0000-000000000000")
        .await;
    assert!(missing.is_err(), "get_message on an unknown id must fail");

    let mut update_meta = HashMap::new();
    update_meta.insert("edited".to_owned(), json!(true));
    let updated_msg = session
        .update_message(created[0].id(), update_meta)
        .await
        .unwrap();
    assert_eq!(updated_msg.metadata().get("edited").unwrap(), &json!(true));

    session.delete().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn peer_metadata_and_configuration_crud() {
    let Some(guard) = guarded_client().await else {
        return;
    };
    let client = guard.client();

    let peer = client.peer("meta-test-peer").build().await.unwrap();

    let mut meta = HashMap::new();
    meta.insert("role".to_owned(), json!("tester"));
    meta.insert("version".to_owned(), json!(2));
    peer.set_metadata(meta).await.unwrap();

    let fetched = peer.get_metadata().await.unwrap();
    assert_eq!(fetched.get("role").unwrap(), &json!("tester"));
    assert_eq!(fetched.get("version").unwrap(), &json!(2));

    let mut config = HashMap::new();
    config.insert("language".to_owned(), json!("en"));
    peer.set_configuration_raw(config).await.unwrap();

    let fetched_config_raw = peer.get_configuration_raw().await.unwrap();
    assert_eq!(fetched_config_raw.get("language").unwrap(), &json!("en"));

    // `update` is a full PUT replace, not a partial merge: keys absent from the
    // new map are dropped, so the prior `role`/`version` must be gone afterwards.
    let mut patch_meta = HashMap::new();
    patch_meta.insert("patched".to_owned(), json!(true));
    peer.update(patch_meta).await.unwrap();
    let after_patch = peer.get_metadata().await.unwrap();
    assert_eq!(after_patch.get("patched").unwrap(), &json!(true));
    assert!(
        !after_patch.contains_key("role"),
        "update() must replace metadata, not merge"
    );
    assert!(
        !after_patch.contains_key("version"),
        "update() must replace metadata, not merge"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn session_clone_and_summaries() {
    let Some(guard) = guarded_client().await else {
        return;
    };
    let client = guard.client();

    let peer = client.peer("clone-test-peer").build().await.unwrap();
    let session = client.session("clone-test-session").build().await.unwrap();
    session.add_peer("clone-test-peer").await.unwrap();

    let msg = peer.message("message before clone").build().unwrap();
    let created = session.add_messages(vec![msg]).await.unwrap();

    let cloned = match session.clone_session().await {
        Ok(c) => c,
        Err(HonchoError::Server { .. }) => {
            eprintln!("skipping clone test: server clone endpoint returned 5xx");
            return;
        }
        Err(e) => panic!("clone_session failed with non-server error: {e}"),
    };
    assert_ne!(cloned.id(), session.id());

    // Same 5xx tolerance as `clone_session` above: a transient server fault must
    // skip the test, not panic. The guard's Drop still deletes the workspace.
    let cloned_with_msg = match session.clone_session_with_message(created[0].id()).await {
        Ok(c) => c,
        Err(HonchoError::Server { .. }) => {
            eprintln!("skipping clone-with-message test: server clone endpoint returned 5xx");
            return;
        }
        Err(e) => panic!("clone_session_with_message failed with non-server error: {e}"),
    };
    assert_ne!(cloned_with_msg.id(), session.id());

    let summaries = session.summaries().await.unwrap();
    assert_eq!(summaries.id, session.id());

    session.delete().await.unwrap();
    cloned.delete().await.unwrap();
    cloned_with_msg.delete().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn session_metadata_and_configuration() {
    let Some(guard) = guarded_client().await else {
        return;
    };
    let client = guard.client();

    let session = client.session("meta-test-session").build().await.unwrap();

    let mut meta = HashMap::new();
    meta.insert("topic".to_owned(), json!("integration"));
    session.set_metadata(meta).await.unwrap();

    let fetched_meta = session.get_metadata().await.unwrap();
    assert_eq!(fetched_meta.get("topic").unwrap(), &json!("integration"));

    let config: SessionConfiguration = serde_json::from_value(json!({
        "summary": {"enabled": true}
    }))
    .unwrap();
    session.set_configuration(&config).await.unwrap();

    let fetched_config = session.get_configuration().await.unwrap();
    assert!(fetched_config.summary.is_some());
    assert_eq!(fetched_config.summary.unwrap().enabled, Some(true));

    session.delete().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn peer_representation_and_context() {
    let Some(guard) = guarded_client().await else {
        return;
    };
    let client = guard.client();

    let peer = client.peer("repr-test-peer").build().await.unwrap();
    let session = client.session("repr-test-session").build().await.unwrap();
    session.add_peer("repr-test-peer").await.unwrap();

    let msg = peer
        .message("I enjoy hiking and outdoor activities")
        .build()
        .unwrap();
    session.add_messages(vec![msg]).await.unwrap();

    let mut delay = Duration::from_millis(500);
    let max_attempts = 5;
    for attempt in 0..max_attempts {
        match peer.representation().await {
            Ok(_) => break,
            Err(e) if attempt + 1 == max_attempts => {
                panic!("representation never became available after {max_attempts} attempts: {e}");
            }
            Err(_) => {
                tokio::time::sleep(delay).await;
                delay *= 2;
            }
        }
    }

    let ctx = peer.context().await.unwrap();
    assert_eq!(ctx.peer_id, peer.id());

    session.delete().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn workspace_metadata_and_configuration() {
    let Some(guard) = guarded_client().await else {
        return;
    };
    let client = guard.client();

    let mut meta = HashMap::new();
    meta.insert("env".to_owned(), json!("integration-test"));
    client.set_metadata(meta).await.unwrap();
    let fetched_meta = client.get_metadata().await.unwrap();
    assert_eq!(fetched_meta.get("env").unwrap(), &json!("integration-test"));

    let mut config = HashMap::new();
    config.insert("feature_x".to_owned(), json!(true));
    client.set_configuration_raw(config).await.unwrap();
    let fetched_config = client.get_configuration_raw().await.unwrap();
    assert_eq!(fetched_config.get("feature_x").unwrap(), &json!(true));
}

#[tokio::test(flavor = "multi_thread")]
async fn session_per_peer_configuration() {
    let Some(guard) = guarded_client().await else {
        return;
    };
    let client = guard.client();

    let session = client.session("peer-cfg-session").build().await.unwrap();
    session.add_peer("peer-cfg-a").await.unwrap();

    let cfg: SessionPeerConfig =
        serde_json::from_value(json!({"observe_me": true, "observe_others": false})).unwrap();
    session
        .set_peer_configuration("peer-cfg-a", &cfg)
        .await
        .unwrap();

    let fetched = session.get_peer_configuration("peer-cfg-a").await.unwrap();
    assert_eq!(fetched.observe_me, Some(true));
    assert_eq!(fetched.observe_others, Some(false));

    session.delete().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn conclusion_query_with_distance_filter() {
    let Some(guard) = guarded_client().await else {
        return;
    };
    let client = guard.client();

    let observer = client.peer("conc-observer").build().await.unwrap();
    let observed_peer = client.peer("conc-observed").build().await.unwrap();
    assert_eq!(observed_peer.id(), "conc-observed");

    let scope = observer.conclusions_of("conc-observed");

    scope
        .create([
            ConclusionCreateParams::new("likes coffee in the morning"),
            ConclusionCreateParams::new("enjoys hiking on weekends"),
            ConclusionCreateParams::new("prefers dark mode in editors"),
        ])
        .await
        .unwrap();

    // Small delay to let the server index the freshly created conclusions.
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Retry the distance-bounded query: indexing is eventually consistent and
    // the endpoint may briefly return a transient 5xx.
    let max_attempts = 5;
    let mut delay = Duration::from_millis(500);
    let mut results = Vec::new();
    let mut last_server_error: Option<String> = None;

    for attempt in 0..max_attempts {
        match scope
            .query("coffee preferences")
            .top_k(5)
            .distance(1.0)
            .send()
            .await
        {
            Ok(r) if !r.is_empty() => {
                results = r;
                break;
            }
            // Empty result: not indexed yet, back off and retry.
            Ok(_) => {}
            // Transient 5xx: tolerate and retry, remembering it in case every
            // attempt fails the same way.
            Err(HonchoError::Server { status, message }) => {
                last_server_error = Some(format!("HTTP {status} {message}"));
            }
            Err(e) => panic!("conclusion query failed: {e}"),
        }
        if attempt + 1 < max_attempts {
            tokio::time::sleep(delay).await;
            delay *= 2;
        }
    }

    if results.is_empty() {
        // A pure server-side fault is tolerated as a skip (the guard still
        // deletes the workspace on return). An empty-but-successful response,
        // however, means the headline distance query is broken: fail loudly.
        if let Some(err) = last_server_error {
            eprintln!("skipping conclusion distance query assert: server returned {err}");
            return;
        }
        panic!(
            "distance query returned no conclusions after {max_attempts} attempts (feature broken)"
        );
    }

    // Contract: the distance-filtered query must surface the coffee conclusion.
    assert!(
        results.iter().any(|c| c.content().contains("coffee")),
        "distance query did not return the expected 'coffee' conclusion"
    );

    // A distance bound only narrows results: an unfiltered query must return at
    // least as many conclusions as the bounded one. Only a transient server-side
    // 5xx (e.g. eventual consistency) is tolerated as a skip; any other error is
    // a real client/logic fault and must fail the test.
    match scope.query("coffee preferences").top_k(5).send().await {
        Ok(unfiltered) => assert!(
            unfiltered.len() >= results.len(),
            "distance filter returned more results than the unfiltered query"
        ),
        Err(HonchoError::Server { status, message }) => {
            eprintln!(
                "skipping unfiltered superset assert: server returned HTTP {status} {message}"
            );
        }
        Err(e) => panic!("unfiltered conclusion query failed: {e}"),
    }

    let page = scope.list().send().await.unwrap();
    let listed = page.items();
    assert!(
        listed.len() >= 3,
        "expected at least the three created conclusions"
    );
    for c in &listed {
        scope.delete(c.id.clone()).await.ok();
    }
}
