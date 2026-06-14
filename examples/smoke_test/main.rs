//! End-to-end smoke test for the `honcho-ai` SDK.
//!
//! Drives every public scenario against a live Honcho API server and reports a
//! pass/fail tally. This binary is the SDK's only end-to-end CI signal, so it
//! is engineered to never report a false green:
//!
//! - `main` returns [`ExitCode`] (never `process::exit`) so the [`WorkspaceGuard`]
//!   always drops and `delete_workspace` always runs — no leaked `smoke-test-*`
//!   workspaces, even on failure.
//! - Any setup error (client build / unreachable server) is recorded via
//!   `report.fail(...)` and yields [`ExitCode::FAILURE`].
//! - Every aborted scenario calls `report.fail(...)`, so a setup explosion shows
//!   red, not green.
//!
//! ## Running
//!
//! Requires a reachable Honcho API server (default `http://localhost:8000`,
//! override with the `HONCHO_API_URL` env var). The smoke test must run on a
//! multi-thread tokio runtime so the cleanup guard can block on `Drop`.
//!
//! ```text
//! HONCHO_API_URL=http://localhost:8000 cargo run --example smoke_test
//! ```

#![allow(clippy::print_stdout, clippy::print_stderr)]

mod chat;
mod conclusions;
mod context;
mod harness;
mod messages;
mod peer;
mod session;
mod workspace;

use std::process::ExitCode;

use harness::{TestReport, WorkspaceGuard};
use honcho_ai::Honcho;

const DEFAULT_BASE_URL: &str = "http://localhost:8000";

#[tokio::main(flavor = "multi_thread")]
async fn main() -> ExitCode {
    // `unwrap_or_else(|_| ...)` here also covers `VarError::NotUnicode`: a
    // corrupt env value falls back to the default rather than being silently
    // dropped by an `Err(_)` arm.
    let base_url = std::env::var("HONCHO_API_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_owned());
    let workspace_id = format!("smoke-test-{}", chrono::Utc::now().timestamp_millis());

    println!("=== honcho-ai SDK Smoke Test ===");
    println!("Server:   {base_url}");
    println!("Workspace: {workspace_id}");
    println!();

    let report = TestReport::new();

    let params = Honcho::builder()
        .base_url(base_url)
        .workspace_id(workspace_id.clone())
        .build();

    let honcho = match Honcho::from_params(params) {
        Ok(c) => c,
        Err(e) => {
            // Record as a failure (not just stderr) so a setup explosion is RED.
            report.fail("setup: create client", &e.to_string());
            println!();
            println!("=== Results: {report} ===");
            return ExitCode::FAILURE;
        }
    };

    if let Err(e) = honcho.force_ensure().await {
        report.fail(
            "setup: reach server",
            &format!("cannot reach {}: {e}", honcho.base_url()),
        );
        eprintln!("Make sure the Honcho API server is running.");
        println!();
        println!("=== Results: {report} ===");
        return ExitCode::FAILURE;
    }
    println!("Workspace ensured.");

    // Construct the guard *after* ensure succeeds: from here on, every exit
    // path drops `_guard` → `delete_workspace` runs.
    let _guard = WorkspaceGuard::new(honcho.clone(), workspace_id);

    // Every scenario takes `&report` and reports its own aborts via
    // `report.fail`, so an aborted scenario always increments `failed`.
    peer::run(&honcho, &report).await;
    if let Err(e) = session::run(&honcho, &report).await {
        report.fail("session: scenario aborted", &e.to_string());
    }
    messages::run(&honcho, &report).await;
    if let Err(e) = chat::run(&honcho, &report).await {
        report.fail("chat: scenario aborted", &e.to_string());
    }
    if let Err(e) = conclusions::run(&honcho, &report).await {
        report.fail("conclusions: scenario aborted", &e.to_string());
    }
    context::run(&honcho, &report).await;
    workspace::run(&honcho, &report).await;

    println!();
    println!("=== Results: {report} ===");

    if report.failed_count() > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
