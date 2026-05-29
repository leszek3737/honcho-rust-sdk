# API Reference — honcho-ai v0.1.5

Complete reference for the Honcho Rust SDK. Every public type, method, and pattern.

## Table of Contents

- [Client (`Honcho`)](#client-honcho)
- [Peer](#peer)
- [Session](#session)
- [Message](#message)
- [Conclusion](#conclusion)
- [DialecticStream](#dialecticstream)
- [Error Handling](#error-handling)
- [Pagination](#pagination)
- [File Upload (`FileSource`)](#file-upload-filesource)
- [Blocking API](#blocking-api)

---

## Client (`Honcho`)

The entry point for all Honcho operations. Wraps an HTTP client, workspace scope, and lazy workspace initialization.

### Construction

```rust
// Quick constructor
let client = Honcho::new("http://localhost:8000", "my-workspace")?;

// Builder with full control
let client = Honcho::from_params(
    Honcho::builder()
        .api_key("sk-...")                        // or HONCHO_API_KEY env
        .base_url("http://localhost:8000")         // or HONCHO_URL env
        .workspace_id("my-workspace")              // or HONCHO_WORKSPACE_ID env
        .timeout(Duration::from_secs(120))
        .max_retries(3)
        .default_headers(headers)
        .default_query(query_params)
        .build(),
)?;

// Using Environment enum
let client = Honcho::from_params(
    Honcho::builder()
        .environment(Environment::Production)     // https://api.honcho.dev
        .build(),
)?;
```

**Resolution order:** explicit builder arg → `HONCHO_*` env var → default.

### Methods

#### `force_ensure() -> Result<()>`
Explicitly ensure the workspace exists on the server. Normally workspace creation is lazy (triggered by the first peer/session call). Use this to pre-create workspaces or catch misconfigurations early.

```rust
client.force_ensure().await?;
```

#### `workspace_id() -> &str`
Returns the workspace ID this client is scoped to.

#### `base_url() -> &Url`
Returns the resolved (normalized) base URL.

#### `peer(id, metadata?, configuration?) -> Result<Peer>`
Get-or-create a peer. Will also ensure the workspace if not done yet.

```rust
let peer = client.peer("alice", None, None).await?;
let peer = client.peer("alice", Some(metadata), Some(config)).await?;
```

#### `session(id, metadata?, peers?, configuration?) -> Result<Session>`
Get-or-create a session.

```rust
use honcho_ai::session::PeerSpec;

let session = client.session("chat-1", None, None, None).await?;
let session = client.session("chat-1", None, Some(vec![
    PeerSpec::Id("alice".into()),
    PeerSpec::WithConfig("bob".into(), config),
]), None).await?;
```

#### `refresh() -> Result<()>`
Refresh workspace state by re-fetching metadata and configuration.

#### Metadata & Configuration

```rust
// Metadata (raw HashMap)
let meta = client.get_metadata().await?;
client.set_metadata(meta).await?;

// Typed configuration
let config = client.get_configuration().await?;
client.set_configuration(&config).await?;

// Raw JSON access (for future/unknown fields)
let raw_config = client.get_configuration_raw().await?;
client.set_configuration_raw(raw_config).await?;
```

#### Listing (paginated)

```rust
// Peers — page 1, 50 items
let page = client.peers().await?;
let page = client.peers_with_filters(filters, page, size, reverse).await?;

// Sessions
let page = client.sessions().await?;
let page = client.sessions_with_filters(filters, page, size, reverse).await?;

// Workspaces (IDs only)
let page = client.workspaces().await?;
```

#### `search(query, limit?, filters?) -> Result<Vec<Message>>`
Search messages across the workspace (semantic search).

```rust
let results = client.search("important topic", None, None).await?;
let results = client.search("topic", Some(5), Some(filters)).await?;
```

#### `queue_status(observer_id?, sender_id?, session_id?) -> Result<QueueStatus>`
Get queue processing status.

```rust
let status = client.queue_status(None, None, None).await?;
```

#### `schedule_dream(observer, session_id?, observed_peer?) -> Result<()>`
Schedule a background memory consolidation task.

```rust
client.schedule_dream("alice", None, None).await?;
client.schedule_dream("alice", Some("chat-1"), Some("bob")).await?;
```

#### `delete_workspace(id) -> Result<()>`
Delete a workspace by ID.

```rust
client.delete_workspace("old-workspace").await?;
```

---

## Peer

Represents a peer (identity) in a Honcho workspace. Provides chat (dialectic reasoning), representation, context, conclusions, search, and metadata management.

### Construction

Always created via `client.peer()` — never constructed directly.

### Identity

```rust
let id = peer.id();                     // &str
let meta = peer.metadata();             // Option<HashMap<String, Value>>
let config = peer.configuration();      // Option<PeerConfig>
```

### Dialectic Chat

#### Non-streaming

```rust
// Simple
let reply: Option<String> = peer.chat("What does Alice like?").await?;

// With full options (session, target, reasoning level)
use honcho_ai::types::dialectic::DialecticOptions;

let opts = DialecticOptions::builder()
    .query("What did Bob say about the project?")
    .target("bob")
    .session_id("chat-1")
    .reasoning_level(honcho_ai::types::dialectic::ReasoningLevel::Medium)
    .build();

let reply = peer.chat_with_options(&opts).await?;
```

Returns `Ok(None)` when the server response has no content.

#### Streaming (SSE)

```rust
use futures_util::StreamExt;

// Simple streaming
let mut stream = peer.chat_stream("Hello").send().await?;
while let Some(chunk) = stream.next().await {
    print!("{}", chunk?);
}

// Full builder
let mut stream = peer.chat_stream("What does Bob think?")
    .target("bob")
    .session_id("chat-1")
    .reasoning_level(ReasoningLevel::High)
    .send()
    .await?;

// Final response after stream completes
let final_resp = stream.final_response();
println!("Full response: {}", final_resp.content());
```

### Representation

The peer's self-model — what it "knows" about itself.

```rust
// Default parameters
let rep = peer.representation().await?;

// With search context
let rep = peer.representation_builder()
    .search_query("hobbies")
    .search_top_k(10)
    .send()
    .await?;
```

### Context (for LLM providers)

Generate context blocks consumable by OpenAI and Anthropic message APIs.

```rust
// Default context
let ctx = peer.context().await?;

// Context from the perspective of another peer
let ctx = peer.context_with_target("bob").await?;

// Full builder with options
let ctx = peer.context_builder()
    .search_query("project alpha")
    .search_top_k(20)
    .target("bob")                // get context about bob
    .send()
    .await?;

// Access formatted context for direct LLM use
println!("{}", ctx.openai);       // OpenAI format
println!("{}", ctx.anthropic);    // Anthropic format
println!("{}", ctx.session_context); // session-level context
```

### Peer Card

A curated list of key facts about a peer.

```rust
let card = peer.get_card().await?;     // Vec<String>
peer.set_card(vec!["friendly".into(), "rustacean".into()]).await?;
```

### Conclusions

Durable facts about peers, formed through the dialectic process.

```rust
// Conclusions about this peer
let scope = peer.conclusions();
let page = scope.list().send().await?;
let created = scope.create(["Likes hiking"]).await?;
scope.delete(conclusion_id).await?;

// Conclusions from this peer about another peer
let cross = peer.conclusions_of("bob");
let page = cross.list().send().await?;
```

### Search & Sessions

```rust
let results = peer.search("topic").await?;
let results = peer.search_with_options(&opts).await?;
let sessions_page = peer.sessions().await?;
```

### Metadata CRUD

```rust
peer.refresh().await?;
let meta = peer.get_metadata().await?;
peer.set_metadata(meta).await?;
peer.update(patch).await?;             // patch update — only changes sent fields
let config = peer.get_configuration().await?;
peer.set_configuration(&config).await?;
let raw = peer.get_configuration_raw().await?;
peer.set_configuration_raw(raw).await?;
```

---

## Session

Represents a conversation session grouping messages between peers.

### Construction

Always created via `client.session()`.

### Identity

```rust
let id = session.id();                 // &str
let active = session.is_active();      // bool
let meta = session.metadata();         // Option<HashMap<String, Value>>
let config = session.configuration();  // Option<SessionConfiguration>
let created = session.created_at();    // &DateTime<Utc>
```

### Messages

#### Creating Messages

Messages are created via `Peer::message()`:

```rust
let msg = peer.message("Hello world!").build()?;
let msg = peer.message("Hello")
    .metadata(json!({"source": "api"}))
    .configuration(json!({"reasoning": true}))
    .build()?;
```

#### Batch Operations

```rust
// Add messages (1-100 at a time)
let msgs: Vec<Message> = session.add_messages(vec![msg1, msg2]).await?;

// Get single message
let msg = session.get_message("msg-id").await?;

// Update message metadata
let msg = session.update_message("msg-id", new_metadata).await?;
```

#### Listing

```rust
let page = session.messages().await?;  // page 1, 50 items
let page = session.messages_with_options(Some(filters), page, size, reverse).await?;
```

The `messages_with_options` method allows paginated listing with filters:
- `filters`: `Option<HashMap<String, Value>>`
- `page`: `u64` (1-based)
- `size`: `u64` (1..=100)
- `reverse`: `bool`

### File Upload

Rust-exclusive feature: typed file sources.

```rust
use honcho_ai::FileSource;

// From bytes (in-memory)
let msgs = session.upload_file(
    FileSource::bytes("doc.pdf", &pdf_data, "application/pdf")
).peer("alice").send().await?;

// From file path (streams from disk, re-opens on retry)
let msgs = session.upload_file(
    FileSource::path("report.txt")
).peer("alice")
 .metadata(json!({"source": "filesystem"}))
 .send()
 .await?;

// From async stream (buffered once, then replayed on retry)
let msgs = session.upload_file(
    FileSource::stream("large.bin", reader, "application/octet-stream")
).peer("alice").send().await?;
```

The upload builder chain requires `.peer(id)` before `.send()`. Optional: `.metadata()`, `.configuration()`, `.created_at()`.

**MIME types:** `text/*`, `application/pdf`, `application/json`, `image/*`, `audio/*`, `video/*`.

### Peer Management

```rust
session.add_peer("bob").await?;
session.add_peers([("alice", config), ("bob", config)]).await?;
session.set_peers(["alice", "bob"]).await?;       // replaces all
session.remove_peers(["bob"]).await?;
let peers = session.peers().await?;                // Vec<PeerResponse>
```

Per-peer configuration:
```rust
let cfg = session.get_peer_configuration("alice").await?;
session.set_peer_configuration("alice", &cfg).await?;
```

> **Note:** `set_peer_configuration` requires the peer to already be in the session (via `add_peer`/`set_peers` first).

### Context, Search & Summaries

```rust
// Session context (for LLM consumption)
let ctx = session.context().await?;
let ctx = session.context_with_options(&opts).await?;

// Or using context_builder
let ctx = session.context_builder()
    .summary(true)
    .peer_target("alice")
    .peer_perspective("bob")
    .send()
    .await?;

println!("{}", ctx.openai);
println!("{}", ctx.anthropic);

// Search within session
let results = session.search("topic").await?;
let results = session.search_with_options(&opts).await?;

// Session summaries
let summaries = session.summaries().await?;

// Peer representation within session context
let rep = session.representation("alice").await?;

// Queue status
let status = session.queue_status(None, None).await?;
```

### Clone & Delete

```rust
let cloned = session.clone_session().await?;
let cloned = session.clone_session_with_message("msg-42").await?; // clone from message point
session.delete().await?;
```

### Metadata CRUD

```rust
session.refresh().await?;
let meta = session.get_metadata().await?;
session.set_metadata(meta).await?;
let config = session.get_configuration().await?;
session.set_configuration(config).await?;
```

---

## Message

A message in a Honcho session. Created via `Peer::message()`.

### Accessors

All fields accessed through methods (not direct field access):

```rust
let id = msg.id();                // &str
let content = msg.content();      // &str
let peer_id = msg.peer_id();      // &str
let session_id = msg.session_id();// &str
let workspace_id = msg.workspace_id(); // &str
let metadata = msg.metadata();    // &HashMap<String, Value>
let created_at = msg.created_at();// &DateTime<Utc>
let token_count = msg.token_count(); // u64
```

### Construction

```rust
let msg: MessageCreate = peer.message("Hello from Rust!")
    .metadata(json!({"timestamp": "2026-01-01T00:00:00Z"}))
    .configuration(json!({"reasoning": true}))
    .build()?;
```

### Traits

- `Debug` — truncated to 50 chars, multibyte-safe
- `Display` — full content text
- `Send + Sync + Clone`

---

## Conclusion

A durable fact about a peer. Managed through `Peer::conclusions()` and `Peer::conclusions_of()`.

```rust
let scope = peer.conclusions();  // ConclusionScope

// List conclusions
let page = scope.list().send().await?;
for conclusion in page.items() {
    println!("{}: {}", conclusion.id(), conclusion.content());
}

// Create
use honcho_ai::ConclusionCreateParams;
let created = scope.create(["Likes hiking in mountains"]).await?;

// Create with optional session association
let params = ConclusionCreateParams::builder()
    .content("Dislikes broccoli")
    .session_id("chat-1")
    .build();
let created = scope.create([params]).await?;

// Delete
scope.delete("conclusion-id").await?;

// Cross-peer conclusions
let cross = peer.conclusions_of("bob");
let page = cross.list().send().await?;  // what this peer concludes about bob
```

---

## DialecticStream

SSE stream adapter for streaming chat responses.

```rust
use futures_util::StreamExt;

let mut stream = peer.chat_stream("Hello").send().await?;

// Stream individual chunks
while let Some(chunk) = stream.next().await {
    match chunk {
        Ok(text) => print!("{text}"),
        Err(e) => eprintln!("Stream error: {e}"),
    }
}

// Access final response after stream completes
let final_response = stream.final_response();
println!("Content: {}", final_response.content());

// Check completion status without consuming
if !stream.is_complete() {
    println!("Stream still active...");
}
```

### `FinalResponse`

```rust
let resp = stream.final_response();
let content = resp.content();    // &str — the full response text
let raw = resp.raw();            // &Value — raw JSON if needed
```

---

## Error Handling

All API methods return `Result<T, HonchoError>` where `HonchoError` is `#[non_exhaustive]`.

### Error Codes

Use `code()` for stable pattern matching:

```rust
match e.code() {
    "not_found" => /* handle missing resource */,
    "authentication_error" => /* bad or missing API key */,
    "rate_limit_exceeded" => /* back off */,
    "validation_error" => /* invalid input */,
    "timeout" | "connection_error" => /* retry */,
    _ => /* unknown or server error */,
}
```

### Error Variants

| Code | HTTP | Description |
|------|------|-------------|
| `bad_request` | 400 | Invalid request |
| `authentication_error` | 401 | Missing/invalid API key |
| `permission_denied` | 403 | Insufficient permissions |
| `not_found` | 404 | Resource not found |
| `conflict` | 409 | Resource already exists |
| `unprocessable_entity` | 422 | Semantic validation failure |
| `rate_limit_exceeded` | 429 | Too many requests |
| `client_error` | 4xx | Other client error |
| `server_error` | 5xx | Server error |
| `timeout` | — | Request timed out |
| `connection_error` | — | Network failure |
| `transport_error` | — | Reqwest transport error |
| `decode_error` | — | Response parsing failure |
| `io_error` | — | Filesystem I/O error |
| `configuration_error` | — | Client configuration issue |
| `validation_error` | — | Client-side validation |

### Utility Methods

```rust
e.code()            // "not_found"
e.status_code()     // Some(404) or None for non-HTTP errors
e.message()         // human-readable message
e.is_retryable()    // true for timeout, connection, 429, 5xx
e.retry_after()     // Some(Duration) for 429 with Retry-After header
```

### Auto-Retry

The HTTP client automatically retries retryable errors: up to 2 retries, 500ms initial delay with exponential backoff. Respects `Retry-After` header. You typically don't need your own retry logic.

---

## Pagination

All list methods return `Page<T>`.

### Properties

```rust
let page: Page<Peer> = client.peers().await?;
page.items();         // &[Peer] — current page items
page.page();          // u64 — current page number (1-based)
page.size();          // u64 — items per page
page.total();         // u64 — total items across all pages
page.pages();         // u64 — total pages
page.has_next();      // bool
```

### Auto-Streaming

```rust
use futures_util::StreamExt;

let stream = page.into_stream();
tokio::pin!(stream);
while let Some(Ok(item)) = stream.next().await {
    // process each item lazily — fetches pages as needed
}
```

### Manual Pagination

```rust
let mut page = client.peers_with_filters(filters, 1, 10, false).await?;
loop {
    for peer in page.items() {
        println!("{}", peer.id);
    }
    if !page.has_next() { break; }
    page = page.next_page().await?;  // Result<Option<Page<T>>>
}
```

### Filters

Filters are `HashMap<String, Value>` — the same format as Python/TS SDKs:

```rust
let mut filters = HashMap::new();
filters.insert("is_active".into(), json!(true));
filters.insert("role".into(), json!("admin"));
let page = client.peers_with_filters(filters, 1, 10, false).await?;
```

**Validation:** `page >= 1`, `size` in `1..=100`, validated client-side before making the request.

---

## File Upload (`FileSource`)

Rust-exclusive typed abstraction for file sources.

```rust
pub enum FileSource {
    /// Upload from in-memory bytes.
    Bytes { filename: String, bytes: Vec<u8>, content_type: String },

    /// Stream from disk — re-opens on retry, unbounded file size.
    Path(std::path::PathBuf),

    /// From an async reader — buffered once, replayed on retry.
    Stream { filename: String, reader: Box<dyn AsyncRead + Send + Unpin>, content_type: String },
}
```

### Constructors

```rust
FileSource::bytes("file.txt", b"hello", "text/plain")
FileSource::path("report.pdf")
FileSource::stream("data.bin", async_reader, "application/octet-stream")
```

### Upload Builder

```rust
session.upload_file(source)
    .peer("alice")                          // required
    .metadata(json!({"note": "important"})) // optional
    .configuration(json!({"process": true}))// optional
    .created_at(Utc::now())                 // optional
    .send()
    .await?;                                // returns Vec<Message>
```

---

## Blocking API

Enable with `features = ["blocking"]`:

```toml
honcho-ai = { version = "0.1", features = ["blocking"] }
```

### Usage

```rust
use honcho_ai::blocking::Honcho;

let client = Honcho::new("http://localhost:8000", "my-workspace")?;
let peer = client.peer("alice")?;
let reply = peer.chat("Hello")?;
let session = client.session("chat-1")?;
session.add_messages(vec![peer.message("Hi").build()?])?;
```

### Import Path

All types are available either via `honcho_ai` (async) or `honcho_ai::blocking` (sync):

| Async | Blocking |
|-------|----------|
| `honcho_ai::Honcho` | `honcho_ai::blocking::Honcho` |
| `honcho_ai::Peer` | `honcho_ai::blocking::Peer` |
| `honcho_ai::Session` | `honcho_ai::blocking::Session` |
| `honcho_ai::Message` | `honcho_ai::blocking::Message` |

Error types are shared — `honcho_ai::error::HonchoError` works with both.

### Important

The blocking API panics if called from within an existing async runtime (e.g., inside `tokio::spawn` or `#[tokio::test]`). Use the async client in those cases.

---

## Builder Pattern Convention

Many types use `bon::Builder` with `#[builder(finish_fn = build)]`:

```rust
let params = Honcho::builder()
    .base_url("...")
    .workspace_id("...")
    .build();

let opts = DialecticOptions::builder()
    .query("...")
    .session_id("...")
    .build();

let ctx_opts = SessionContextOptions::builder()
    .peer_perspective("alice")
    .peer_target("bob")
    .build();
ctx_opts.validate()?; // required when perspective/target is set
```
