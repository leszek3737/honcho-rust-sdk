use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};

use honcho_ai::Honcho;

pub struct TestReport {
    passed: AtomicUsize,
    failed: AtomicUsize,
    current_scenario: std::sync::Mutex<String>,
}

impl TestReport {
    pub fn new() -> Self {
        Self {
            passed: AtomicUsize::new(0),
            failed: AtomicUsize::new(0),
            current_scenario: std::sync::Mutex::new(String::new()),
        }
    }

    pub fn scenario(&self, name: &str) {
        if let Ok(mut cur) = self.current_scenario.lock() {
            name.clone_into(&mut *cur);
        }
        println!("--- {name} ---");
    }

    pub fn pass(&self, name: &str) {
        self.passed.fetch_add(1, Ordering::Relaxed);
        println!("  \u{2713} {name}");
    }

    pub fn fail(&self, name: &str, err: &str) {
        self.failed.fetch_add(1, Ordering::Relaxed);
        println!("  \u{2717} {name}: {err}");
    }

    pub fn failed_count(&self) -> usize {
        self.failed.load(Ordering::Relaxed)
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

pub struct WorkspaceGuard {
    client: Honcho,
    workspace_id: String,
    should_cleanup: std::sync::atomic::AtomicBool,
}

impl WorkspaceGuard {
    pub fn new(client: Honcho, workspace_id: String) -> Self {
        Self {
            client,
            workspace_id,
            should_cleanup: std::sync::atomic::AtomicBool::new(true),
        }
    }

    #[allow(dead_code)]
    pub fn workspace_id(&self) -> &str {
        &self.workspace_id
    }

    #[allow(dead_code)]
    pub fn client(&self) -> &Honcho {
        &self.client
    }

    #[allow(dead_code)]
    pub fn preserve(&self) {
        self.should_cleanup.store(false, Ordering::Relaxed);
    }
}

impl Drop for WorkspaceGuard {
    fn drop(&mut self) {
        if !self.should_cleanup.load(Ordering::Relaxed) {
            return;
        }
        let client = self.client.clone();
        let ws_id = self.workspace_id.clone();
        tokio::task::block_in_place(|| {
            let rt = tokio::runtime::Handle::current();
            match rt.block_on(client.delete_workspace(&ws_id)) {
                Ok(()) => eprintln!("  cleaned up workspace {ws_id}"),
                Err(e) => eprintln!("  warning: failed to delete workspace {ws_id}: {e}"),
            }
        });
    }
}
