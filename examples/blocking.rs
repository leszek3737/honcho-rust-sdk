#![allow(clippy::print_stdout)]
//! Blocking (synchronous) API usage.
//!
//! The blocking client owns its own runtime, so it returns a `Configuration` error
//! if constructed inside an existing async runtime (e.g. a `#[tokio::main]` fn).
//!
//! Run with `cargo run --example blocking --features blocking`

use honcho_ai::blocking::Honcho;

fn main() -> honcho_ai::error::Result<()> {
    // Hardcoded URL is the documented convention for examples.
    let honcho = Honcho::new("http://localhost:8000", "blocking-demo")?;
    // Eager opt-in: peer()/session() lazily ensure_workspace on first use, so this
    // call is only needed to create the workspace up front.
    honcho.force_ensure()?;

    let peer = honcho.peer("user-1").build()?;
    let session = honcho.session("sess-1").build()?;

    // Positional `None, None` are the optional config/metadata args (defaults here).
    session.add_messages(vec![peer.message("Hello from blocking!").build()?])?;

    // The deriver runs asynchronously, so a chat right after add_messages may not yet
    // reflect the fresh message (first runs often return None).
    if let Some(response) = peer.chat("What do you know about me?")? {
        println!("Response: {response}");
    } else {
        println!("No response yet");
    }

    Ok(())
}
