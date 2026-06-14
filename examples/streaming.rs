#![allow(clippy::print_stdout)]
//! Streaming dialectic: `chat_stream` rendered live, chunk by chunk.
//!
//! Run with `cargo run --example streaming`

use std::io::Write;

use futures_util::TryStreamExt;
use honcho_ai::Honcho;

#[tokio::main]
async fn main() -> honcho_ai::error::Result<()> {
    let honcho = Honcho::from_params(
        Honcho::builder()
            .base_url("http://localhost:8000")
            .workspace_id("streaming-demo")
            .build(),
    )?;

    let peer = honcho.peer("user-1").build().await?;

    // Positional args after the query: metadata, configuration (both default).
    let mut stream = peer.chat_stream("Tell me a story").send().await?;

    // Lock stdout once for the whole drain — relocking per chunk would serialize
    // needlessly. We `write!` + `flush()` each chunk so the reply appears live
    // instead of buffering until the trailing newline (stdout is line-buffered).
    let mut out = std::io::stdout().lock();
    // `chunk?` propagates a mid-stream error: the SDK terminates the stream on the
    // first failure, so anything printed so far is partial content, not the reply.
    while let Some(text) = stream.try_next().await? {
        write!(out, "{text}")?;
        out.flush()?;
    }
    writeln!(out)?;

    // `final_response()` is the defining `DialecticStream` feature: the adapter
    // accumulates every chunk, so after the drain it holds the full reply.
    let final_response = stream.final_response();
    writeln!(out, "\n--- final response ---\n{final_response}")?;

    Ok(())
}
