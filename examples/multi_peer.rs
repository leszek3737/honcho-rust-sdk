#![allow(clippy::print_stdout)]
//! Multi-peer session with peer management.
//!
//! Demonstrates adding multiple peers to a session, exchanging messages,
//! and using per-peer dialectic chat (both a session summary and a
//! peer-targeted query so every peer is exercised, not just the speaker).
//!
//! Run with `cargo run --example multi_peer`

use honcho_ai::Honcho;
use honcho_ai::types::dialectic::DialecticOptions;

#[tokio::main]
async fn main() -> honcho_ai::error::Result<()> {
    let honcho = Honcho::new("http://localhost:8000", "multi-peer-demo")?;

    // The three peers are independent — create them concurrently so the
    // (lazy, OnceCell-guarded) workspace ensure runs once and the peer
    // round-trips overlap instead of going one after another.
    let (alice, bob, carol) = tokio::try_join!(
        honcho.peer("alice").build(),
        honcho.peer("bob").build(),
        honcho.peer("carol").build(),
    )?;

    let session = honcho.session("group-chat").build().await?;

    session.set_peers([&alice, &bob, &carol]).await?;

    // Batch both messages into a single add_messages call (one round-trip).
    session
        .add_messages(vec![
            alice.message("Hi everyone!").build()?,
            bob.message("Hey Alice!").build()?,
        ])
        .await?;

    // Scope the dialectic to this session via session.id() rather than
    // re-typing the literal — a typo would silently target a non-existent
    // session (empty representation, no error).
    let response = alice
        .chat_with_options(
            &DialecticOptions::builder()
                .query("Summarize the conversation")
                .session_id(session.id())
                .build(),
        )
        .await?;
    if let Some(text) = response {
        println!("Alice's response: {text}");
    } else {
        println!("Alice has no response yet (messages may still be processing)");
    }

    // A second, peer-targeted chat: carol asks about alice's perspective.
    // `target` selects whose representation to read — the only field the
    // first chat left unused.
    let carol_view = carol
        .chat_with_options(
            &DialecticOptions::builder()
                .query("What is Alice interested in?")
                .session_id(session.id())
                .target(alice.id())
                .build(),
        )
        .await?;
    if let Some(text) = carol_view {
        println!("Carol's view of Alice: {text}");
    } else {
        println!("Carol has no view of Alice yet (messages may still be processing)");
    }

    let peers = session.peers().await?;
    println!("Session has {} peer(s)", peers.len());

    Ok(())
}
