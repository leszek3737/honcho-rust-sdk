#![allow(clippy::print_stdout)]
//! Conclusion lifecycle example: create → list → query → delete.
//!
//! Run with `cargo run --example conclusions`.
//!
//! This file demonstrates the conclusion API surface. It compiles but does
//! not run cleanly without a live Honcho server.

use honcho_ai::{ConclusionCreateParams, Honcho};

#[tokio::main]
async fn main() -> honcho_ai::error::Result<()> {
    let honcho = Honcho::new("http://localhost:8000", "demo-ws")?;

    let peer = honcho.peer("alice").build().await?;

    // Self-scoped conclusions (observer = observed = alice)
    let scope = peer.conclusions();

    // Create one conclusion
    let created = scope
        .create([ConclusionCreateParams::new("Alice likes dark mode")])
        .await?;
    println!("created: {created:?}");

    // Create with session scope (builder has `on(String, into)`, so `&str` works)
    let session_scoped = scope
        .create([ConclusionCreateParams::builder()
            .content("Alice prefers async/await")
            .session_id("sess-1")
            .build()])
        .await?;
    if let Some(first) = session_scoped.first() {
        println!("session-scoped conclusion id: {}", first.id());
    }

    // Cross-peer conclusions (observer = alice, observed = bob). `conclusions_of`
    // is a pure scope constructor (no API call), so `bob` must exist before the
    // create below actually hits the server.
    honcho.peer("bob").build().await?;
    let cross = peer.conclusions_of("bob");
    let cross_created = cross
        .create([ConclusionCreateParams::new("Bob is a morning person")])
        .await?;
    if let Some(first) = cross_created.first() {
        println!("cross-peer conclusion id: {}", first.id());
    }

    // List conclusions (paginated)
    let page = scope.list().page(1).size(10).send().await?;
    println!("list: total={}, page={}", page.total(), page.page());
    for conclusion in page.items() {
        // `items()` yields the raw response rows, so fields are accessed directly.
        println!("  - {} {}", conclusion.id, conclusion.content);
    }

    // Semantic query
    let results = scope
        .query("programming preferences")
        .top_k(5)
        .send()
        .await?;
    println!("query returned {} results", results.len());

    // Scoped representation
    let rep = cross
        .representation()
        .search_query("personality")
        .max_conclusions(20)
        .send()
        .await?;
    println!("representation: {rep}");

    // Delete every conclusion we created so repeated runs don't litter the
    // workspace. `delete` is keyed by conclusion id (not scope-bound), so the
    // same call removes self-scoped, session-scoped, and cross-peer rows alike.
    for conclusion in created.iter().chain(&session_scoped).chain(&cross_created) {
        scope.delete(conclusion.id()).await?;
        println!("deleted {}", conclusion.id());
    }

    Ok(())
}
