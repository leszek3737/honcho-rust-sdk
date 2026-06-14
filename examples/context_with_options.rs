#![allow(clippy::print_stdout)]
//! Session context with custom options: summary toggle + session-scoped representation.
//!
//! Demonstrates `context_with_options` for controlling what context is returned,
//! and converting the result to the `OpenAI` chat-message format with the
//! correct assistant peer.
//!
//! Run with `cargo run --example context_with_options`

use honcho_ai::Honcho;
use honcho_ai::types::session::SessionContextOptions;

#[tokio::main]
async fn main() -> honcho_ai::error::Result<()> {
    let honcho = Honcho::from_params(
        Honcho::builder()
            .base_url("http://localhost:8000")
            .workspace_id("context-demo")
            .build(),
    )?;

    // Two distinct roles so the OpenAI conversion below is meaningful: the user
    // asks, the assistant answers.
    let user = honcho.peer("user-1").build().await?;
    let assistant = honcho.peer("assistant-1").build().await?;
    let session = honcho.session("sess-1").build().await?;

    session
        .add_messages(vec![
            user.message("Hello from context example!").build()?,
            assistant
                .message("Hello back from the assistant!")
                .build()?,
        ])
        .await?;

    // Default context. `session.context()` already returns the summary (the
    // builder default is `summary = true`), so there is no separate "with
    // summary" call to make — see the `summary(false)` contrast below.
    let ctx = session.context().await?;
    println!("Default context: {} messages", ctx.messages.len());

    // Contrast against the default: explicitly *dropping* the summary is the
    // observable change. `.summary(true)` would just repeat the default.
    let ctx_no_summary = session
        .context_with_options(&SessionContextOptions::builder().summary(false).build())
        .await?;
    println!(
        "No summary: {} messages, summary present: {}",
        ctx_no_summary.messages.len(),
        ctx_no_summary.summary.is_some(),
    );
    // The default context keeps the summary, e.g. its content.
    println!(
        "Default summary: {}",
        ctx.summary
            .as_ref()
            .map_or("<none>", |s| s.content.as_str()),
    );

    // `limit_to_session(true)` constrains the *peer representation* to this
    // session rather than the whole workspace — it does not change the message
    // list. The same request is also expressible idiomatically via
    // `session.context_builder().limit_to_session(true).send()`.
    let ctx_session_only = session
        .context_with_options(
            &SessionContextOptions::builder()
                .limit_to_session(true)
                .build(),
        )
        .await?;
    println!(
        "Session-scoped representation: {}",
        ctx_session_only
            .peer_representation
            .as_deref()
            .unwrap_or("<none>"),
    );

    // `to_openai` maps messages from the given peer to `role: "assistant"` and
    // every other peer to `role: "user"`, so pass the *assistant* peer here —
    // passing the user peer would invert the roles. Note the entry count is not
    // the turn count: any peer_representation / peer_card / summary is prepended
    // as a leading system entry.
    let openai = ctx.to_openai(&assistant);
    println!("OpenAI format: {} entries", openai.len());

    Ok(())
}
