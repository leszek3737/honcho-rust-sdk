# Examples — honcho-ai

Real-world patterns for using the Honcho Rust SDK.

## Complete Agent Setup

A complete example showing workspace → peer → session → chat flow:

```rust
use honcho_ai::{Honcho, FileSource};
use serde_json::json;

#[tokio::main]
async fn main() -> honcho_ai::error::Result<()> {
    // 1. Connect
    let client = Honcho::from_params(
        Honcho::builder()
            .base_url("http://localhost:8000")
            .workspace_id("my-agent-app")
            .build(),
    )?;

    // 2. Ensure workspace exists
    client.force_ensure().await?;

    // 3. Create user peer and agent peer
    let user = client.peer("user-123", None, None).await?;
    let agent = client.peer("assistant", None, None).await?;

    // 4. Start a conversation session
    let session = client.session("chat-2026-01-01", None, None, None).await?;

    // 5. Add peers to session
    session.add_peers([
        ("user-123".into(), Default::default()),
        ("assistant".into(), Default::default()),
    ]).await?;

    // 6. Add user message
    let user_msg = user.message("I really enjoy hiking in the mountains!")
        .metadata(json!({"source": "chat"}))
        .build()?;
    session.add_messages(vec![user_msg]).await?;

    // 7. Query the memory
    let reply = agent.chat("What outdoor activities does the user enjoy?").await?;
    if let Some(text) = reply {
        println!("Agent reply: {text}");
    }

    // 8. Check what the agent knows about the user
    let rep = agent.representation_builder()
        .search_query("hobbies")
        .search_top_k(10)
        .send()
        .await?;
    println!("Agent's representation of user: {rep}");

    Ok(())
}
```

## Multi-Peer Conversation

Managing a group chat with multiple peers:

```rust
use honcho_ai::session::{PeerSpec, SessionPeerConfig};

#[tokio::main]
async fn main() -> honcho_ai::error::Result<()> {
    let client = Honcho::new("http://localhost:8000", "group-chat")?;

    let alice = client.peer("alice", None, None).await?;
    let bob = client.peer("bob", None, None).await?;
    let charlie = client.peer("charlie", None, None).await?;

    // Configure observation: bob observes alice, charlie observes both
    let session = client.session("team-discussion", None, Some(vec![
        PeerSpec::Id("alice".into()),
        PeerSpec::WithConfig("bob".into(), SessionPeerConfig {
            observe_me: Some(true),
            observe_others: Some(false),
        }),
        PeerSpec::WithConfig("charlie".into(), SessionPeerConfig {
            observe_me: None,
            observe_others: Some(true),
        }),
    ]), None).await?;

    // Alice sends a message
    session.add_messages(vec![
        alice.message("The deployment goes live tomorrow!").build()?
    ]).await?;

    // What does Bob know?
    let bob_ctx = bob.context_with_target("alice").await?;
    println!("Bob knows about Alice: {}", bob_ctx.openai);

    // What does Charlie see overall?
    let charlie_view = charlie.representation().await?;
    println!("Charlie's worldview: {charlie_view}");

    Ok(())
}
```

## File Upload

Upload a PDF and ask about its contents:

```rust
use honcho_ai::FileSource;

#[tokio::main]
async fn main() -> honcho_ai::error::Result<()> {
    let client = Honcho::new("http://localhost:8000", "docs")?;
    let agent = client.peer("assistant", None, None).await?;
    let session = client.session("doc-review", None, None, None).await?;

    // Upload a PDF from disk
    let msgs = session.upload_file(
        FileSource::path("contract.pdf")
    ).peer("user-123")
     .metadata(json!({"document_type": "contract"}))
     .send()
     .await?;

    println!("Uploaded {} messages from contract", msgs.len());

    // Or from bytes (e.g., received via HTTP)
    let pdf_bytes = reqwest::get("https://example.com/doc.pdf")
        .await?
        .bytes()
        .await?;

    session.upload_file(
        FileSource::bytes("doc.pdf", &pdf_bytes, "application/pdf")
    ).peer("user-123").send().await?;

    // Ask about the uploaded document
    let reply = agent.chat_with_options(
        &honcho_ai::types::dialectic::DialecticOptions::builder()
            .query("Summarize the key points from the contract")
            .session_id("doc-review")
            .build()
    ).await?;

    if let Some(summary) = reply {
        println!("Summary: {summary}");
    }

    Ok(())
}
```

## Streaming Chat with SSE

Real-time token-by-token response:

```rust
use futures_util::StreamExt;
use honcho_ai::types::dialectic::ReasoningLevel;

#[tokio::main]
async fn main() -> honcho_ai::error::Result<()> {
    let client = Honcho::new("http://localhost:8000", "streaming-demo")?;
    let agent = client.peer("assistant", None, None).await?;

    // Simple streaming
    let mut stream = agent.chat_stream("Tell me a story about Rust").send().await?;

    print!("Response: ");
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(text) => print!("{text}"),   // incremental token
            Err(e) => eprintln!("\nStream error: {e}"),
        }
    }

    // Full response after streaming
    let final_resp = stream.final_response();
    println!("\n\nFull response: {}", final_resp.content());

    // Advanced: targeted streaming with reasoning level
    let mut stream = agent.chat_stream("What does Alice think about Bob?")
        .target("alice")                         // query from Alice's perspective
        .session_id("group-chat")
        .reasoning_level(ReasoningLevel::High)    // thorough reasoning
        .send()
        .await?;

    while let Some(Ok(token)) = stream.next().await {
        // render token to UI...
    }

    Ok(())
}
```

## Conclusions — Building a Peer's Fact Profile

```rust
#[tokio::main]
async fn main() -> honcho_ai::error::Result<()> {
    let client = Honcho::new("http://localhost:8000", "memory-demo")?;
    let agent = client.peer("assistant", None, None).await?;
    let user = client.peer("user-123", None, None).await?;

    // After a conversation, schedule memory consolidation
    client.schedule_dream("assistant", None, None).await?;

    // Later: check what conclusions were formed
    let scope = agent.conclusions_of("user-123");
    let page = scope.list().send().await?;

    println!("Assistant's conclusions about user:");
    for fact in page.items() {
        println!("  - {} (id: {})", fact.content(), fact.id());
    }

    // Manually add a conclusion
    agent.conclusions_of("user-123").create([
        honcho_ai::ConclusionCreateParams::builder()
            .content("Prefers dark mode in all applications")
            .build()
    ]).await?;

    // Check the peer card (curated key facts)
    let card = user.get_card().await?;
    println!("\nUser peer card: {:?}", card);

    // Update the card
    user.set_card(vec![
        "Rust developer".into(),
        "Prefers dark mode".into(),
        "Works in fintech".into(),
    ]).await?;

    Ok(())
}
```

## Pagination — Processing Large Lists

```rust
use futures_util::StreamExt;

#[tokio::main]
async fn main() -> honcho_ai::error::Result<()> {
    let client = Honcho::new("http://localhost:8000", "pagination-demo")?;

    // Method 1: Auto-fetch all pages as a stream
    let page = client.peers().await?;
    println!("{} total peers across {} pages", page.total(), page.pages());

    let stream = page.into_stream();
    tokio::pin!(stream);
    let mut count = 0;
    while let Some(Ok(peer)) = stream.next().await {
        println!("Processing peer: {}", peer.id);
        count += 1;
    }
    assert_eq!(count as u64, page.total());

    // Method 2: Manual pagination with custom size
    let mut page = client.sessions_with_filters(
        std::collections::HashMap::new(),  // no filters
        1,               // start at page 1
        10,              // 10 items per page
        false,           // ascending order
    ).await?;

    loop {
        println!("=== Page {}, {} items ===", page.page(), page.items().len());
        for session in page.items() {
            println!("  Session: {} (active: {})", session.id, session.is_active);
        }

        if !page.has_next() { break; }
        page = page.next_page().await?.expect("has_next was true");
    }

    Ok(())
}
```

## Error Handling Patterns

```rust
use honcho_ai::error::HonchoError;

#[tokio::main]
async fn main() -> honcho_ai::error::Result<()> {
    let client = Honcho::new("http://localhost:8000", "error-demo")?;

    // Pattern 1: Match on error code
    match client.peer("nonexistent").await {
        Ok(peer) => { /* use peer */ },
        Err(e) => match e.code() {
            "not_found" => eprintln!("Create the peer first!"),
            "authentication_error" => eprintln!("Check your API key"),
            "rate_limit_exceeded" => {
                if let Some(wait) = e.retry_after() {
                    tokio::time::sleep(wait).await;
                }
            }
            code => eprintln!("Error: {code} — {}", e.message()),
        },
    }

    // Pattern 2: Check retryability
    async fn with_retry<F, T>(mut f: F) -> honcho_ai::error::Result<T>
    where F: FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = honcho_ai::error::Result<T>>>>,
    {
        loop {
            match f().await {
                Ok(v) => return Ok(v),
                Err(e) if e.is_retryable() => {
                    let delay = e.retry_after().unwrap_or(std::time::Duration::from_secs(1));
                    eprintln!("Retrying after {:?}...", delay);
                    tokio::time::sleep(delay).await;
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
    }

    // Usage
    let peer = with_retry(|| Box::pin(client.peer("alice"))).await?;

    Ok(())
}
```

## Blocking API (non-async)

```rust
use honcho_ai::blocking::Honcho;

fn main() -> honcho_ai::error::Result<()> {
    let client = Honcho::new("http://localhost:8000", "sync-demo")?;

    let peer = client.peer("alice", None, None)?;
    let session = client.session("chat-1", None, None, None)?;

    session.add_messages(vec![
        peer.message("Hello from synchronous Rust!").build()?
    ])?;

    let reply = peer.chat("What did I just say?");

    match reply {
        Ok(Some(text)) => println!("Reply: {text}"),
        Ok(None) => println!("No content in response"),
        Err(e) => eprintln!("Error: {}", e.message()),
    }

    Ok(())
}
```
