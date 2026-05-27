# honcho-ai

[![Crates.io](https://img.shields.io/crates/v/honcho-ai.svg)](https://crates.io/crates/honcho-ai)
[![Docs](https://docs.rs/honcho-ai/badge.svg)](https://docs.rs/honcho-ai)
[![CI](https://github.com/leszek3737/honcho-rust-sdk/actions/workflows/ci.yml/badge.svg)](https://github.com/leszek3737/honcho-rust-sdk/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE.md)
[![MSRV](https://img.shields.io/badge/MSRV-1.88.0-orange.svg)](https://blog.rust-lang.org/2025/03/20/Rust-1.88.0.html)

Rust SDK for [Honcho](https://github.com/plastic-labs/honcho) — AI agent memory and social cognition infrastructure.

> **Status:** Beta — full API parity with Python/TypeScript SDKs. Production-ready for async Rust.

## What is Honcho?

Honcho is an AI agent memory platform that provides persistent, contextual memory and social cognition for LLM agents. It enables your agents to:

- **Remember** past interactions across sessions and users
- **Understand** social context — who knows what about whom
- **Reason** dialectically — query and synthesize knowledge across peers
- **Evolve** self-representation and peer models over time

This crate is a native Rust client for the Honcho API (v3), with **100% parity** with the official Python and TypeScript SDKs.

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
honcho-ai = "0.1"
tokio = { version = "1", features = ["full"] }
```

Or via cargo:

```bash
cargo add honcho-ai
```

## Quickstart

```rust
use honcho_ai::Honcho;

#[tokio::main]
async fn main() -> honcho_ai::error::Result<()> {
    // Connect to Honcho (local or cloud)
    let client = Honcho::from_params(
        Honcho::builder()
            .base_url("http://localhost:8000")
            .workspace_id("my-agent")
            .build(),
    )?;

    // Create a peer (agent or user identity)
    let peer = client.peer("alice", None, None).await?;

    // Create a session and add a message
    let session = client.session("chat-1", None, None, None).await?;
    session.add_messages(vec![
        peer.message("Hello! My name is Alice and I like hiking.")
            .build()?
    ]).await?;

    // Ask the dialectic layer — Honcho reasons about what it knows
    let reply = peer.chat("What does Alice like?").await?;
    if let Some(text) = reply {
        println!("Alice likes: {text}");
    }

    // Streaming chat for real-time response delivery
    use futures_util::StreamExt;
    let mut stream = peer.chat_stream("Tell me more about Alice")
        .send()
        .await?;
    while let Some(chunk) = stream.next().await {
        print!("{}", chunk?);
    }

    Ok(())
}
```

## Key Concepts

| Concept | Description |
|---------|-------------|
| **Workspace** | Isolated container for peers, sessions, and all memory data |
| **Peer** | An identity (user or AI agent) with persistent memory, a dynamic representation, and a peer card of key facts |
| **Session** | A conversation context grouping messages between peers |
| **Message** | A unit of communication belonging to a peer within a session |
| **Dialectic Chat** | LLM-powered reasoning over all available context — the "query the memory" layer |
| **Conclusion** | A durable fact about a peer, formed through the dialectic process |
| **Dream** | Background memory consolidation task, triggered by `schedule_dream` |

## Core API

### Client (`Honcho`)

```rust
let client = Honcho::new("http://localhost:8000", "my-workspace")?;

// Workspace management
client.force_ensure().await?;          // ensure workspace exists
client.get_metadata().await?;          // fetch metadata
client.set_metadata(map).await?;       // set metadata
client.get_configuration().await?;     // typed config
client.delete_workspace("old").await?; // delete workspace

// Paginated listing
let peers_page = client.peers().await?;
let sessions_page = client.sessions().await?;
let workspaces_page = client.workspaces().await?;

// Search & dream
let results = client.search("topic", None, None).await?;
client.schedule_dream("alice", None, None).await?;
```

### Peer

```rust
let peer = client.peer("alice", None, None).await?;

// Dialectic chat
let reply = peer.chat("What does Alice like?").await?;
let reply = peer.chat_with_options(&opts).await?;
let stream = peer.chat_stream("Hello").send().await?;

// Representation & context
let rep = peer.representation().await?;
let ctx = peer.context().await?;
let ctx = peer.context_with_target("bob").await?;

// Peer card (key facts)
let card = peer.get_card().await?;
peer.set_card(vec!["friendly".into(), "rust-dev".into()]).await?;

// Conclusions
peer.conclusions().list().send().await?;
peer.conclusions().create(["Likes hiking"]).await?;
peer.conclusions().delete(conclusion_id).await?;
let cross = peer.conclusions_of("bob"); // cross-peer conclusions

// Search & sessions
let results = peer.search("topic").await?;
let page = peer.sessions().await?;

// Metadata CRUD
peer.refresh().await?;
peer.get_metadata().await?;
peer.set_metadata(map).await?;
peer.update(patch).await?;            // patch update
```

### Session

```rust
let session = client.session("chat-1", None, None, None).await?;

// Messages (batch up to 100 per call)
let msgs = session.add_messages(vec![msg]).await?;
let page = session.messages().await?;
let msg = session.get_message("msg-1").await?;
let msg = session.update_message("msg-1", metadata).await?;

// File upload — typed sources (Rust-only feature)
use honcho_ai::FileSource;
let msgs = session.upload_file(
    FileSource::bytes("doc.pdf", data, "application/pdf")
).peer("alice").send().await?;

let msgs = session.upload_file(
    FileSource::path("report.txt")
).peer("alice").send().await?;

// Peer management
session.add_peer("bob").await?;
session.set_peers(["alice", "bob"]).await?;
session.remove_peers(["bob"]).await?;

// Context & search
let ctx = session.context().await?;
let results = session.search("topic").await?;
let summaries = session.summaries().await?;

// Clone & delete
let cloned = session.clone_session().await?;
session.delete().await?;
```

### Error Handling

```rust
use honcho_ai::error::HonchoError;

match client.peer("alice", None, None).await {
    Ok(peer) => { /* ... */ },
    Err(e @ HonchoError::NotFound { .. }) => {
        eprintln!("Peer not found: {}", e.message());
        // e.code() returns "not_found"
        // e.status_code() returns Some(404)
        // e.is_retryable() returns false
    },
    Err(e) if e.is_retryable() => {
        // Timeout, connection error, or 429/5xx — retry
        tokio::time::sleep(e.retry_after().unwrap_or(Duration::from_secs(1))).await;
    },
    Err(e) => return Err(e),
}
```

The SDK auto-retries transient errors (2 retries, exponential backoff, respects `Retry-After`). You don't need to implement your own retry loop for timeouts, connection errors, 429, or 5xx responses.

## Features

| Feature | Default | Description |
|---------|---------|-------------|
| `rustls-tls` | ✓ | TLS via rustls (recommended) |
| `native-tls` | | TLS via platform-native backend |
| `blocking` | | Synchronous API for non-async contexts |
| `tracing` | | Emit [`tracing`](https://docs.rs/tracing) spans for all API calls |

### Blocking API

Enable the `blocking` feature for synchronous usage:

```toml
honcho-ai = { version = "0.1", features = ["blocking"] }
```

```rust
use honcho_ai::blocking::Honcho;

let client = Honcho::new("http://localhost:8000", "my-workspace")?;
let peer = client.peer("alice", None, None)?;
let reply = peer.chat("Hello")?;
```

> **Note:** The blocking API uses an internal `tokio` runtime. It will panic if you call it from within an existing async context — use the async API instead.

## Rust-Only Features

These APIs have no equivalent in the Python/TypeScript SDKs:

| Feature | Description |
|---------|-------------|
| `FileSource` enum | Typed file upload sources: `bytes()`, `path()`, `stream()` |
| `Honcho::force_ensure()` | Explicit workspace creation (normally lazy via `ensure_workspace`) |
| `Peer::conclusions()` / `Peer::conclusions_of()` | Scoped conclusion handles with lazy evaluation |
| `DialecticStream::is_complete()` | Check stream completion without consuming |

## Pagination

All list endpoints return a `Page<T>` with 1-based page numbers:

```rust
// Default: page 1, 50 items
let page = client.peers().await?;
println!("Page {} of {} ({} total)", page.page(), page.pages(), page.total());
for peer in page.items() {
    println!("  - {}", peer.id);
}

// Auto-fetch all pages
use futures_util::StreamExt;
let stream = page.into_stream();
tokio::pin!(stream);
while let Some(Ok(item)) = stream.next().await {
    // process each item lazily
}

// Manual pagination
let mut page = client.sessions_with_filters(filters, 1, 10, false).await?;
while page.has_next() {
    page = page.next_page().await?;
}
```

## Environment Variables

| Variable | Field | Default |
|----------|-------|---------|
| `HONCHO_API_KEY` | API key | `None` |
| `HONCHO_URL` / `HONCHO_API_URL` | Base URL | `https://api.honcho.dev` |
| `HONCHO_WORKSPACE_ID` | Workspace ID | `"default"` |

Explicit builder arguments take precedence over environment variables.

## Parity with Official SDKs

| Feature | Python | TypeScript | Rust |
|---------|--------|------------|------|
| Peer CRUD | ✓ | ✓ | ✓ |
| Session CRUD | ✓ | ✓ | ✓ |
| Messages (batch) | ✓ | ✓ | ✓ |
| Dialectic chat (text) | ✓ | ✓ | ✓ |
| Streaming chat (SSE) | ✓ | ✓ | ✓ |
| Representation | ✓ | ✓ | ✓ |
| Peer card | ✓ | ✓ | ✓ |
| Conclusions | ✓ | ✓ | ✓ |
| File upload | ✓ | ✓ | ✓ |
| Context (OpenAI/Anthropic) | ✓ | ✓ | ✓ |
| API key auth | ✓ | ✓ | ✓ |
| Paginated listing | ✓ | ✓ | ✓ |
| Typed file sources | — | — | ✓ |
| Scoped conclusions | — | — | ✓ |
| `force_ensure` | — | — | ✓ |

## Requirements

- **Rust** 1.88.0 or later (edition 2024)
- **Tokio** async runtime (multi-thread)
- Honcho server (local or [api.honcho.dev](https://api.honcho.dev))

## Links

- [Repository](https://github.com/leszek3737/honcho-rust-sdk)
- [API Documentation](https://docs.rs/honcho-ai)
- [Honcho Server](https://github.com/plastic-labs/honcho)
- [OpenAPI Spec](./tests/schemas/openapi.json)
- [Migration Guide](./MIGRATION.md)
- [Changelog](./CHANGELOG.md)

## License

MIT — see [LICENSE.md](LICENSE.md).

---

*Built by [Leszek](https://github.com/leszek3737). Not affiliated with Plastic Labs — community-maintained with the intent to contribute upstream.*
