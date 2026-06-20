//! Shared helpers for the integration test harness.
//!
//! Crate-level lint allows live in `main.rs` (single source); do not duplicate
//! them here.

use std::env;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use honcho_ai::Honcho;
use tokio::runtime::Handle;

/// Monotonic per-process counter guaranteeing distinct workspace ids even when
/// two calls observe the same wall-clock timestamp (the millisecond-suffix
/// approach this replaced collided under parallel test execution).
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Returns a per-call unique token of the form `{pid}-{nanos}-{n}`.
///
/// - `pid` disambiguates across processes (parallel test binaries / CI shards),
/// - `nanos` adds wall-clock entropy across runs,
/// - the atomic `n` guarantees uniqueness *within* a process regardless of
///   clock resolution — this is what actually makes the id collision-free.
fn unique_token() -> String {
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{pid}-{nanos}-{n}")
}

/// Resolves the test server base URL.
///
/// Mirrors the SDK precedence (`HONCHO_URL` wins over `HONCHO_API_URL`) but
/// defaults to a local server instead of the production environment.
pub fn api_base_url() -> String {
    env::var("HONCHO_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| env::var("HONCHO_API_URL").ok().filter(|s| !s.is_empty()))
        .unwrap_or_else(|| "http://localhost:8000".to_string())
}

/// Resolves the API key from `HONCHO_API_KEY`, treating empty as unset —
/// consistent with [`Honcho::from_params`].
pub fn maybe_api_key() -> Option<String> {
    env::var("HONCHO_API_KEY").ok().filter(|s| !s.is_empty())
}

/// Returns a fresh, collision-free workspace id for a single test.
pub fn unique_workspace_id() -> String {
    format!("rust-int-test-{}", unique_token())
}

/// Builds a client and verifies the server is reachable.
///
/// Returns `None` (logging the cause) when either the client cannot be built or
/// the server cannot be reached, so tests soft-skip instead of failing when no
/// server is configured.
pub async fn try_client() -> Option<Honcho> {
    let base_url = api_base_url();
    let ws_id = unique_workspace_id();

    let params = Honcho::builder()
        .base_url(&base_url)
        .maybe_api_key(maybe_api_key())
        .workspace_id(&ws_id)
        .build();

    let client = match Honcho::from_params(params) {
        Ok(client) => client,
        Err(e) => {
            eprintln!("skipping integration test: could not build client: {e}");
            return None;
        }
    };

    match client.force_ensure().await {
        Ok(()) => Some(client),
        Err(e) => {
            eprintln!("skipping integration test: could not connect to server: {e}");
            None
        }
    }
}

/// RAII guard that deletes the client's workspace on drop, isolating each test.
///
/// # Runtime requirement
///
/// Every test constructing a `WorkspaceGuard` MUST be annotated
/// `#[tokio::test(flavor = "multi_thread")]`. The `Drop` teardown awaits the
/// asynchronous `delete_workspace` via [`tokio::task::block_in_place`] +
/// [`Handle::block_on`], which requires a multi-thread runtime; `new` asserts
/// this up front so a forgotten annotation fails loudly in the test body
/// instead of mid-`Drop`. `Drop` itself never panics — with no runtime it logs
/// a loud LEAK warning rather than aborting the process. The previous
/// fire-and-forget `Handle::spawn` approach silently leaked workspaces because
/// the spawned task was never polled to completion before the runtime shut down.
pub struct WorkspaceGuard {
    client: Honcho,
    workspace_id: String,
    cleanup: AtomicBool,
}

impl WorkspaceGuard {
    /// Wraps `client`, capturing its workspace id for cleanup on drop.
    ///
    /// Panics with an actionable message if called outside a multi-thread
    /// runtime: the `Drop` teardown uses `block_in_place`, which would
    /// otherwise panic *during unwinding* and abort the process, swallowing
    /// the test's real failure. Checking here surfaces a forgotten
    /// `flavor = "multi_thread"` annotation in the test body instead.
    pub fn new(client: Honcho) -> Self {
        assert_eq!(
            Handle::current().runtime_flavor(),
            tokio::runtime::RuntimeFlavor::MultiThread,
            "WorkspaceGuard requires #[tokio::test(flavor = \"multi_thread\")]"
        );
        let workspace_id = client.workspace_id().to_string();
        Self {
            client,
            workspace_id,
            cleanup: AtomicBool::new(true),
        }
    }

    /// Borrows the wrapped client.
    pub fn client(&self) -> &Honcho {
        &self.client
    }

    /// Opts this guard out of workspace cleanup (e.g. to inspect server state
    /// after a failure). Opt-in API: tests may legitimately never call it.
    #[allow(dead_code)]
    pub fn preserve(&self) {
        self.cleanup.store(false, Ordering::Relaxed);
    }
}

impl Drop for WorkspaceGuard {
    fn drop(&mut self) {
        if !self.cleanup.load(Ordering::Relaxed) {
            return;
        }
        let client = &self.client;
        let ws_id = &self.workspace_id;
        // Never panic in Drop: a panic during unwinding aborts the process and
        // swallows the test's real failure. The multi-thread requirement is
        // enforced loudly at construction (see `new`). Here we degrade
        // gracefully but LOUDLY — a missing runtime means the workspace leaked,
        // which must stay visible, never a silent skip.
        let Ok(handle) = Handle::try_current() else {
            eprintln!(
                "  WARNING: no Tokio runtime in WorkspaceGuard::drop; workspace {ws_id} LEAKED"
            );
            return;
        };
        // `block_in_place` requires the multi-thread runtime asserted in `new`.
        tokio::task::block_in_place(|| {
            handle.block_on(async {
                // The server refuses to delete a workspace while active sessions
                // remain (HTTP 409 "Delete all sessions first"), so drain sessions
                // before the workspace. Without this, every test that creates a
                // session leaks its workspace on the server.
                match delete_all_sessions(client).await {
                    Ok(0) => {}
                    Ok(n) => eprintln!("  deleted {n} session(s) in workspace {ws_id}"),
                    Err(e) => {
                        eprintln!("  warning: failed to delete sessions in {ws_id}: {e}");
                    }
                }
                match client.delete_workspace(ws_id).await {
                    Ok(()) => eprintln!("  cleaned up workspace {ws_id}"),
                    Err(e) => eprintln!("  warning: failed to delete workspace {ws_id}: {e}"),
                }
            });
        });
    }
}

/// Deletes every session in the client's workspace, returning the count removed.
///
/// Collects all ids across every page *first*, then deletes — deleting mid-walk
/// would renumber the remaining pages and let `next_page()` skip the sessions
/// that shifted forward. `session(id).build()` re-asserts the session (a cheap
/// upsert) to obtain a handle, then deletes it — there is no by-id delete on the
/// public client surface.
///
/// Deletion is best-effort: a single failing session is logged and skipped
/// rather than `?`-aborting, since one flaky delete would otherwise leave the
/// rest intact and re-trigger the 409 that blocks workspace teardown.
async fn delete_all_sessions(client: &Honcho) -> honcho_ai::error::Result<usize> {
    let mut ids = Vec::new();
    let mut page = client.sessions().await?;
    loop {
        ids.extend(page.items_ref().iter().map(|s| s.id.clone()));
        match page.next_page().await? {
            Some(next) => page = next,
            None => break,
        }
    }
    let mut deleted = 0;
    for id in &ids {
        match client.session(id.clone()).build().await {
            Ok(session) => match session.delete().await {
                Ok(()) => deleted += 1,
                Err(e) => eprintln!("  warning: failed to delete session {id}: {e}"),
            },
            Err(e) => eprintln!("  warning: failed to resolve session {id} for delete: {e}"),
        }
    }
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::unique_workspace_id;

    /// 1000 sequential ids must all be distinct (no server required). Guards
    /// against the timestamp-collision flakiness that motivated the atomic
    /// counter.
    #[test]
    fn unique_workspace_id_yields_distinct_ids() {
        let ids: HashSet<String> = (0..1000).map(|_| unique_workspace_id()).collect();
        assert_eq!(ids.len(), 1000, "all 1000 generated ids must be distinct");
    }
}
