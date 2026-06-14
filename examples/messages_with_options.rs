#![allow(clippy::print_stdout)]
//! Messages with filters: pagination, metadata, and search.
//!
//! Demonstrates adding messages with metadata, paginating the message list
//! with `messages_with_options` + `page.has_next()`, searching, and updating
//! message metadata.
//!
//! Run with `cargo run --example messages_with_options`

use std::collections::HashMap;

use honcho_ai::{Honcho, MessageSearchOptions};

#[tokio::main]
async fn main() -> honcho_ai::error::Result<()> {
    let honcho = Honcho::new("http://localhost:8000", "messages-demo")?;

    let peer = honcho.peer("user-1").build().await?;
    let session = honcho.session("sess-1").build().await?;

    let mut meta = HashMap::new();
    meta.insert("tag".into(), "important".into());

    let msg = peer
        .message("Important announcement")
        .metadata(meta) // moved: `meta` is unused afterwards
        .build()?;

    let created = session.add_messages(vec![msg]).await?;
    println!("Created {} message(s)", created.len());

    // Default listing (page 1, size 50).
    let page = session.messages().await?;
    println!(
        "Session has {} messages (page {})",
        page.total(), // total across all pages, no per-item clone
        page.page()
    );

    // Pagination demo: force size = 1 so multiple messages span multiple pages.
    // Args: filters (None), page (1-based), size, reverse.
    let first_page = session.messages_with_options(None, 1, 1, false).await?;
    println!(
        "First page: {} item(s), has_next = {}",
        first_page.raw_items().len(), // length without cloning/transforming
        first_page.has_next()
    );
    if let Some(next) = first_page.next_page().await? {
        println!(
            "Second page: {} item(s), has_next = {}",
            next.raw_items().len(),
            next.has_next()
        );
    } else {
        println!("No further pages");
    }

    let results = session.search("announcement").await?;
    println!("Search returned {} result(s)", results.len());

    let search_opts = MessageSearchOptions::builder()
        .query("important")
        .limit(5)
        .build();
    let filtered = session.search_with_options(&search_opts).await?;
    println!("Filtered search returned {} result(s)", filtered.len());

    if let Some(first) = created.first() {
        let mut update_meta = HashMap::new();
        update_meta.insert("reviewed".into(), true.into());
        let updated = session.update_message(first.id(), update_meta).await?;
        println!("Updated message {}", updated.id());
    }

    Ok(())
}
