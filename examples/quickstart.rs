#![allow(clippy::print_stdout)]
//! Basic usage: create a peer, start a session, send messages, and get a response.
//!
//! Run with `cargo run --example quickstart`

use honcho_ai::Honcho;

#[tokio::main]
async fn main() -> honcho_ai::error::Result<()> {
    // Simplest constructor: base URL + workspace id (uses HONCHO_API_KEY if set).
    let honcho = Honcho::new("http://localhost:8000", "quickstart-demo")?;

    // Positional `None`s are the optional config/metadata args (left at defaults here).
    let peer = honcho.peer("user-1").build().await?;
    let session = honcho.session("sess-1").build().await?;

    session
        .add_messages(vec![peer.message("Hello, Honcho!").build()?])
        .await?;

    // The deriver runs asynchronously, so a chat right after add_messages may not yet
    // reflect the fresh message (first runs often return None). To scope the reply to
    // this session, use `chat_with_options` with `DialecticOptions::session_id`.
    if let Some(text) = peer.chat("What do you know about me?").await? {
        println!("Response: {text}");
    } else {
        println!("No response yet (messages may still be processing)");
    }

    Ok(())
}
