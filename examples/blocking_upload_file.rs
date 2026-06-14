#![allow(clippy::print_stdout)]
//! Upload a file to a session with the blocking (synchronous) API.
//!
//! Mirrors `examples/upload.rs` but without async/await: it ensures the
//! uploading peer is a session member, then uploads in-memory bytes as a
//! `text/plain` file and prints how many messages the upload created.
//!
//! Run with `cargo run --example blocking_upload_file --features blocking`

use honcho_ai::FileSource;
use honcho_ai::blocking::Honcho;

fn main() -> honcho_ai::error::Result<()> {
    let honcho = Honcho::new("http://localhost:8000", "blocking-file-demo")?;
    honcho.force_ensure()?;

    let peer = honcho.peer("user-1").build()?;
    let session = honcho.session("sess-1").build()?;

    // The uploading peer must be a member of the session before the upload.
    session.add_peer(peer.id())?;

    // The array literal converts straight into the `Vec<u8>` the API needs.
    let source = FileSource::bytes("hello.txt", b"Hello from a file!", "text/plain");
    let messages = session.upload_file(source).peer(peer.id()).send()?;

    println!("Uploaded {} message(s)", messages.len());

    Ok(())
}
