# Code Style & Conventions — honcho-ai

## Builder Pattern
All param structs ending in `Params` use `bon::Builder` with `#[builder(finish_fn = build)]`.

## Wrapper Pattern
Public types (`Honcho`, `Peer`, `Session`, `Message`, `Conclusion`) wrap `Arc<Inner>` with accessor methods. Never expose inner fields directly. Example: `msg.id()`, `msg.content()`, `msg.metadata()` — NOT `msg.id`, `msg.content`.

## No unwrap/expect/panic in library code
Enforced: `#[deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]`. Use `Result` everywhere. Tests may `#![allow(...)]` these.

## Error handling
- `HonchoError` is `#[non_exhaustive]`. Match on `code()` (returns `&'static str` like "bad_request", "not_found") not variants.
- Error type alias: `pub type Result<T> = std::result::Result<T, HonchoError>;`

## Edition 2024
All impl blocks may need `unsafe` markers for unsafe trait impls.

## Formatting
- rustfmt edition 2021, `reorder_imports = true`
- clippy: pedantic + cargo warnings, specific denies on unwrap/expect/panic/dbg_macro/print_stdout/print_stderr

## Env var resolution (HonchoParams)
Resolution order: explicit builder arg → env var → default

| Env var | Field | Default |
|---------|-------|---------|
| `HONCHO_API_KEY` | `api_key` | None |
| `HONCHO_URL` | `base_url` | `https://api.honcho.dev` |
| `HONCHO_WORKSPACE_ID` | `workspace_id` | `"default"` |

## Key Gotchas
- Blocking API panics if called inside an async runtime
- `Page::next_page()` returns `Result<Option<Page<T>>>` — `?` required
- `SessionContextOptions::validate()?` must be called after `.build()` when `peer_perspective` or `peer_target` is set
- `DialecticStream::final_response()` returns `FinalResponse` struct, access text via `.content`
- Message methods use accessors only
- File upload uses multipart streaming, not in-memory buffering
- `FileSource` enum (`bytes`, `path`, `stream`) has no equivalent in Python/TS SDKs

## Test patterns
- Fixture tests: `tests/fixtures/{SchemaName}/{min|max}.json` → OpenAPI validation + serde roundtrip via `schema_tests!` macro
- Wiremock tests: HTTP mocking with `wiremock` crate
- Integration tests: skip gracefully via `try_client()` when no server; print to stderr
- Compile-time assertions: `tests/compile_assertions.rs`
- All test files allow clippy unwrap/expect/panic since denied in library
