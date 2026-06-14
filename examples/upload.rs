#![allow(clippy::print_stdout)]
//! Upload a file to a session.
//!
//! `FileSource` accepts only the MIME types `text/plain`, `application/pdf`, and
//! `application/json`.
//!
//! Run with `cargo run --example upload`

use honcho_ai::{FileSource, Honcho};

#[tokio::main]
async fn main() -> honcho_ai::error::Result<()> {
    let honcho = Honcho::from_params(
        Honcho::builder()
            .base_url("http://localhost:8000")
            .workspace_id("upload-demo")
            .build(),
    )?;

    let peer = honcho.peer("user-1").build().await?;
    let session = honcho.session("sess-1").build().await?;

    // The uploading peer must be a member of the session.
    session.add_peer(peer.id()).await?;

    let source = FileSource::bytes("hello.txt", b"Hello from a file!", "text/plain");
    let messages = session.upload_file(source).peer(peer.id()).send().await?;

    println!("Uploaded {} message(s)", messages.len());
    for message in &messages {
        println!("  message id: {}", message.id());
    }

    Ok(())
}
