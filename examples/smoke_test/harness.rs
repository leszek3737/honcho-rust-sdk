use std::fmt;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use honcho_ai::Honcho;
use tokio::runtime::Handle;
use tokio::time::Duration;

/// Maximum time the [`WorkspaceGuard`] waits for `delete_workspace` during
/// `Drop`, so a hung server cannot wedge the whole smoke run on exit.
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(30);

/// Aggregated pass/fail tally for the smoke suite.
///
/// Uses interior mutability (atomic counters + a mutex for the current
/// scenario name) so every scenario `run()` takes `&self`/`&TestReport`
/// instead of `&mut`. The suite is sequential today, but `&self` keeps the
/// call sites uniform and leaves room for concurrent scenarios.
pub struct TestReport {
    passed: AtomicUsize,
    failed: AtomicUsize,
    current_scenario: Mutex<String>,
}

impl TestReport {
    pub fn new() -> Self {
        Self {
            passed: AtomicUsize::new(0),
            failed: AtomicUsize::new(0),
            current_scenario: Mutex::new(String::new()),
        }
    }

    pub fn scenario(&self, name: &str) {
        if let Ok(mut cur) = self.current_scenario.lock() {
            name.clone_into(&mut cur);
        }
        println!("--- {name} ---");
    }

    pub fn pass(&self, name: &str) {
        self.passed.fetch_add(1, Ordering::Relaxed);
        println!("  \u{2713} {name}");
    }

    pub fn fail(&self, name: &str, err: &str) {
        self.failed.fetch_add(1, Ordering::Relaxed);
        // Prefix the failing test with the active scenario so interleaved CI
        // logs stay attributable.
        let scenario = self
            .current_scenario
            .lock()
            .map(|cur| cur.clone())
            .unwrap_or_default();
        if scenario.is_empty() {
            println!("  \u{2717} {name}: {err}");
        } else {
            println!("  \u{2717} [{scenario}] {name}: {err}");
        }
    }

    pub fn failed_count(&self) -> usize {
        self.failed.load(Ordering::Relaxed)
    }
}

impl Default for TestReport {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for TestReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let p = self.passed.load(Ordering::Relaxed);
        let fl = self.failed.load(Ordering::Relaxed);
        let total = p + fl;
        write!(f, "{p}/{total} passed, {fl} failed")
    }
}

/// RAII guard that deletes the throwaway smoke-test workspace on `Drop`.
///
/// Because `main` returns an [`ExitCode`](std::process::ExitCode) instead of
/// calling `process::exit`, this guard runs even when the suite fails, so no
/// `smoke-test-*` workspace is leaked.
///
/// `Drop` needs a multi-thread tokio runtime: it uses `block_in_place` +
/// `Handle::block_on`, which is only valid on a multi-thread runtime. On a
/// `current_thread` runtime (or outside tokio entirely) it cannot block, so it
/// degrades to an `eprintln!` warning rather than panicking during unwind.
pub struct WorkspaceGuard {
    client: Honcho,
    workspace_id: String,
}

impl WorkspaceGuard {
    pub fn new(client: Honcho, workspace_id: String) -> Self {
        Self {
            client,
            workspace_id,
        }
    }
}

impl Drop for WorkspaceGuard {
    fn drop(&mut self) {
        let ws_id = &self.workspace_id;
        // A multi-thread runtime handle is required to block here. Anything
        // else (current_thread, or no runtime) must not panic in Drop.
        let Ok(handle) = Handle::try_current() else {
            eprintln!(
                "  warning: no tokio runtime in Drop; workspace {ws_id} not cleaned up \
                 (multi-thread runtime required)"
            );
            return;
        };
        tokio::task::block_in_place(|| {
            let cleanup =
                tokio::time::timeout(CLEANUP_TIMEOUT, self.client.delete_workspace(ws_id));
            match handle.block_on(cleanup) {
                Ok(Ok(())) => eprintln!("  cleaned up workspace {ws_id}"),
                Ok(Err(e)) => eprintln!("  warning: failed to delete workspace {ws_id}: {e}"),
                Err(_) => eprintln!(
                    "  warning: timed out deleting workspace {ws_id} after {}s",
                    CLEANUP_TIMEOUT.as_secs()
                ),
            }
        });
    }
}
