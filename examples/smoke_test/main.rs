#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stdout,
    clippy::print_stderr
)]

mod chat;
mod conclusions;
mod context;
mod harness;
mod messages;
mod peer;
mod session;
mod workspace;

use harness::{TestReport, WorkspaceGuard};
use honcho_ai::Honcho;

#[tokio::main]
async fn main() {
    let base_url = match std::env::var("HONCHO_API_URL") {
        Ok(url) => url,
        Err(_) => "http://localhost:8000".to_owned(),
    };
    let workspace_id = format!("smoke-test-{}", chrono::Utc::now().timestamp_millis());

    println!("=== honcho-ai SDK Smoke Test ===");
    println!("Server:   {base_url}");
    println!("Workspace: {workspace_id}");
    println!();

    let params = Honcho::builder()
        .base_url(base_url)
        .workspace_id(workspace_id.clone())
        .build();

    let honcho = match Honcho::from_params(params) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to create client: {e}");
            std::process::exit(1);
        }
    };

    match honcho.force_ensure().await {
        Ok(()) => println!("Workspace ensured."),
        Err(e) => {
            eprintln!("Cannot reach server at {}: {e}", honcho.base_url());
            eprintln!("Make sure the Honcho API server is running.");
            std::process::exit(1);
        }
    }

    let _guard = WorkspaceGuard::new(honcho.clone(), workspace_id);
    let mut report = TestReport::new();

    peer::run(&honcho, &mut report).await;
    if let Err(e) = session::run(&honcho, &mut report).await {
        eprintln!("  session scenario aborted: {e}");
    }
    messages::run(&honcho, &mut report).await;
    if let Err(e) = chat::run(&honcho, &mut report).await {
        eprintln!("  chat scenario aborted: {e}");
    }
    if let Err(e) = conclusions::run(&honcho, &mut report).await {
        eprintln!("  conclusions scenario aborted: {e}");
    }
    context::run(&honcho, &mut report).await;
    workspace::run(&honcho, &mut report).await;

    println!();
    println!("=== Results: {report} ===");

    let failed = report.failed_count();
    if failed > 0 {
        std::process::exit(1);
    }
}
