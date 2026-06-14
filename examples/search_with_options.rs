#![allow(clippy::print_stdout)]
//! Search with options: workspace-level and peer-level search with filters.
//!
//! Demonstrates searching messages across the workspace, within a session,
//! and scoped to a peer with custom limit and `filters` options. The
//! `filters` map scopes a search to messages matching arbitrary metadata.
//!
//! Run with `cargo run --example search_with_options`

use std::collections::HashMap;

use honcho_ai::{Honcho, MessageSearchOptions};

#[tokio::main]
async fn main() -> honcho_ai::error::Result<()> {
    let honcho = Honcho::new("http://localhost:8000", "search-demo")?;

    let peer = honcho.peer("user-1").build().await?;
    let session = honcho.session("sess-1").build().await?;

    session
        .add_messages(vec![
            peer.message("Rust is a systems programming language")
                .build()?,
            peer.message("I enjoy hiking on weekends")
                .metadata(HashMap::from([("topic".into(), "hobbies".into())]))
                .build()?,
            peer.message("Tokio is the async runtime for Rust")
                .build()?,
        ])
        .await?;

    let ws_results = honcho.search("Rust programming").build().await?;
    println!("Workspace search: {} result(s)", ws_results.len());

    let sess_results = session.search("hiking").await?;
    println!("Session search: {} result(s)", sess_results.len());

    let peer_results = peer.search("Rust").await?;
    println!("Peer search: {} result(s)", peer_results.len());

    // Peer search with a higher result limit.
    let peer_search_opts = MessageSearchOptions::builder()
        .query("programming")
        .limit(20)
        .build();
    let peer_filtered = peer.search_with_options(&peer_search_opts).await?;
    println!("Peer filtered search: {} result(s)", peer_filtered.len());

    // Session search constrained by a metadata filter (the example's namesake):
    // only messages tagged `topic = hobbies` are searched. The query term
    // matches the seeded "weekends" content above.
    let filters = HashMap::from([("topic".into(), "hobbies".into())]);
    let sess_search_opts = MessageSearchOptions::builder()
        .query("weekends")
        .filters(filters)
        .limit(5)
        .build();
    let sess_filtered = session.search_with_options(&sess_search_opts).await?;
    println!("Session filtered search: {} result(s)", sess_filtered.len());

    Ok(())
}
