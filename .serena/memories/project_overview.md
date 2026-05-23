# Project Overview — honcho-ai

## Purpose
Rust SDK for Honcho — AI agent memory and social cognition infrastructure. Wraps the Honcho REST API (v3).

## Tech stack
- **Language**: Rust, Edition 2024, MSRV 1.88
- **HTTP**: reqwest 0.12 with rustls-tls (default) or native-tls
- **Async**: tokio (multi-thread runtime)
- **Serde**: serde + serde_json for DTOs
- **Builders**: bon 3 for all builder patterns
- **Time**: chrono (with serde + clock), httpdate
- **Streaming**: async-stream, futures-util, tokio-util

## Features
- `rustls-tls` (default) — TLS via rustls
- `native-tls` — TLS via native backend
- `blocking` — Sync API facade with internal tokio runtime
- `tracing` — Emit tracing spans on public async methods

## Architecture
Single crate (`honcho-ai`), not a workspace. Key modules:
- `client.rs` — `Honcho` entry point (Arc-wrapped, lazy workspace ensure)
- `peer.rs` — `Peer` wrapper (chat, representation, conclusions, etc.)
- `session.rs` — `Session` wrapper (messages, peers, uploads, clone, delete)
- `conclusion.rs` — `Conclusion` + `ConclusionScope` CRUD
- `message.rs` — `Message` wrapper with accessor methods
- `types/` — All request/response DTOs
- `http/` — HttpClient (retry/backoff), routes, SSE, decode
- `blocking/` — Sync facade (feature-gated)
- `error.rs` — `HonchoError` (non_exhaustive) with `code()` method

## API version
Base path is `/v3/`. All route builders in `src/http/routes.rs`.
