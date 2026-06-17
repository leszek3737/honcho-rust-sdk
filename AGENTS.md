# AGENTS.md — honcho-ai (Rust SDK)


## Working Principles

**Tradeoff:** These guidelines bias toward caution over speed. For trivial tasks, use judgment.

### 1. Think Before Coding

**Don't assume. Don't hide confusion. Surface tradeoffs.**

Before implementing:
- State your assumptions explicitly. If uncertain, ask.
- If multiple interpretations exist, present them - don't pick silently.
- If a simpler approach exists, say so. Push back when warranted.
- If something is unclear, stop. Name what's confusing. Ask.

### 2. Simplicity First

**Minimum code that solves the problem. Nothing speculative.**

- No features beyond what was asked.
- No abstractions for single-use code.
- No "flexibility" or "configurability" that wasn't requested.
- No error handling for impossible scenarios.
- If you write 200 lines and it could be 50, rewrite it.

Ask yourself: "Would a senior engineer say this is overcomplicated?" If yes, simplify.

### 3. Surgical Changes

**Touch only what you must. Clean up only your own mess.**

When editing existing code:
- Don't "improve" adjacent code, comments, or formatting.
- Don't refactor things that aren't broken.
- Match existing style, even if you'd do it differently.
- If you notice unrelated dead code, mention it - don't delete it.

When your changes create orphans:
- Remove imports/variables/functions that YOUR changes made unused.
- Don't remove pre-existing dead code unless asked.

The test: Every changed line should trace directly to the user's request.

### 4. Goal-Driven Execution

**Define success criteria. Loop until verified.**

Transform tasks into verifiable goals:
- "Add validation" → "Write tests for invalid inputs, then make them pass"
- "Fix the bug" → "Write a test that reproduces it, then make it pass"
- "Refactor X" → "Ensure tests pass before and after"

For multi-step tasks, state a brief plan:
```
1. [Step] → verify: [check]
2. [Step] → verify: [check]
3. [Step] → verify: [check]
```

Strong success criteria let you loop independently. Weak criteria ("make it work") require constant clarification.

## Build & Test Commands

```sh
cargo build
cargo test                              # all tests (unit + integration; int tests skip if no server)
cargo test --lib                        # unit tests only
cargo test --test '*_types'             # schema validation + roundtrip tests (no server needed)
cargo test --test integration           # integration tests (needs `HONCHO_API_URL`)
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo doc --no-deps
```

Pre-commit order that matters: `fmt -> clippy -> test`.

## Architecture

**Single crate** (`honcho-ai`), not a workspace. Edition 2024, MSRV 1.88.

```
src/
  lib.rs              # re-exports, deny/forbid attrs
  client.rs           # Honcho — top-level entry point (Arc<Inner>, lazy workspace ensure)
  peer.rs             # Peer — chat, representation, context, card, conclusions, search
  session.rs          # Session — messages, peers, upload, context, clone, delete
  conclusion.rs       # Conclusion + ConclusionScope — CRUD for conclusions
  message.rs          # Message — wrapper with accessor methods (not field access)
  dialectic_stream.rs # SSE stream adapter for chat streaming
  upload.rs           # FileSource enum (bytes, path, stream)
  error.rs            # HonchoError (non_exhaustive), parse_retry_after, from_response
  types/              # All request/response DTOs, pagination, validation
  http/               # HttpClient (retry/backoff), routes (API v3), SSE, decode
  blocking/           # Sync facade (feature-gated), internal tokio runtime
```

API base path is `/v3/`. All route builders live in `src/http/routes.rs`.

## Conventions

- **Builders**: `bon::Builder` with `#[builder(finish_fn = build)]` on all param struct names ending in `Params`.
- **Wrapper pattern**: Public types (`Honcho`, `Peer`, `Session`, `Message`, `Conclusion`) use `Arc<Inner>` with accessor methods. Never expose inner fields directly.
- **No unwrap/expect/panic in library code**: enforced by `#[deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]`. Use `Result` everywhere. Tests may `#![allow(...)]` these.
- **Don't use `#[allow(...)]` to bypass the deny lints in library code.** A "validated above" `Option::expect()` is a code smell — restructure instead. Prefer `let Some(x) = opt else { return Err(...) }` (let-else) or `opt.ok_or_else(|| ...)?` so the impossibility is enforced by the type, not an escape hatch. `#[allow]` on a clippy lint is acceptable only when the lint is a genuine false positive (e.g. `trivially_copy_pass_by_ref` on a serde `skip_serializing_if` fn, `mismatching_type_param_order` on `Page<T, T>`) — add a comment when the reason isn't obvious.
- **`#[non_exhaustive]`** on `HonchoError`. Match on `code()` (machine-readable string) not variants.
- **Edition 2024** — all impl blocks may need `unsafe` markers for unsafe trait impls.
- **Always run after every change**: `cargo fmt --check && cargo fmt` (if needed) then `cargo clippy --all-targets --all-features -- -D warnings`. Fix all warnings. Only pre-existing warnings may remain.
- **Never rewrite pushed history**: `git commit --amend`, rebase, squash, and force-push are only for local, un-pushed commits. Once a commit has been pushed to a remote, do **not** amend/rewrite it — add a new commit on top instead.

## Env Var Resolution

`HonchoParams` resolution order: explicit builder arg → env var → default.

| Env var | Field | Default |
|---------|-------|---------|
| `HONCHO_API_KEY` | `api_key` | None |
| `HONCHO_URL` / `HONCHO_API_URL` | `base_url` | `https://api.honcho.dev` |
| `HONCHO_WORKSPACE_ID` | `workspace_id` | `"default"` |

Integration tests use `HONCHO_API_URL` (default `http://localhost:8000`) and `HONCHO_API_KEY`.

## Gotchas

- **Blocking API returns `HonchoError::Configuration` if called inside an async runtime** (`src/blocking/runtime.rs`). Use the async `Honcho` client instead. Streaming-chat iterators (`ChatStreamIterator::next`) and `BlockingIter::try_next` also return `Err(Configuration)` rather than panicking when driven from an async context.
- **`Page::next_page()` returns `Result<Option<Page<T>>>`** — the `?` is required. `into_stream()` auto-fetches pages.
- **`SessionContextOptions::validate()?`** must be called after `.build()` when `peer_perspective` or `peer_target` is set.
- **`DialecticStream::final_response()`** returns `FinalResponse` struct — access text via `.content`, not directly.
- **Message** methods use accessors: `.id()`, `.content()`, `.metadata()`, etc. — not `.id`, `.content` fields.
- **`FileSource`** enum (`bytes`, `path`, `stream`) for uploads. No equivalent in Python/TS SDKs.
- **file upload** uses multipart streaming, not in-memory buffering.

## Test Patterns

- **Fixture tests**: `tests/fixtures/{SchemaName}/{min|max}.json` → OpenAPI validation + serde roundtrip via `schema_tests!` macro. See `tests/session_types.rs` for the pattern.
- **Wiremock tests**: HTTP-level tests using `wiremock` crate. See `tests/wire_format_peers.rs`.
- **Integration tests**: live-server tests in `tests/integration/main.rs`. Skip gracefully via `try_client()` when no server is reachable. Print `eprintln!("skipping integration test: ...")` to stderr.
- **Compile-time assertions**: `tests/compile_assertions.rs` — `Send + Sync + Clone` bounds on all public types.
- All test files `#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]` since these are denied in the library crate.

## Retry Behavior

`HttpClient` retries timeout/connection errors and `429 | 500 | 502 | 503 | 504` responses. Max 2 retries, 500ms initial backoff with exponential delay. Respects `Retry-After` header.

## Serena — LSP Code Intelligence

Serena provides symbol-level code navigation, search, and refactoring. Use these tools for codebase exploration and editing.

### Exploration

| Task | Tool | Example |
|------|------|---------|
| Find a symbol by name | `find_symbol` | `find_symbol(name_path_pattern="Honcho", depth=1)` |
| Get file overview | `get_symbols_overview` | `get_symbols_overview(relative_path="src/client.rs")` |
| Search for patterns | `search_for_pattern` | `search_for_pattern(substring_pattern="pub async fn")` |
| Find declarations | `find_declaration` | `find_declaration(relative_path="src/peer.rs", regex="(chat)\(")` |
| Find references | `find_referencing_symbols` | `find_referencing_symbols(name_path="Honcho/new", relative_path="src/client.rs")` |
| Find implementations | `find_implementations` | `find_implementations(name_path="my_trait", relative_path="src/...")` |

### Editing

| Task | Tool | Notes |
|------|------|-------|
| Rename symbol safely | `rename_symbol` | Handles entire codebase, safer than find-and-replace |
| Replace symbol body | `replace_symbol_body` | Replace function/method implementation |
| Insert before/after symbol | `insert_before_symbol` / `insert_after_symbol` | Insert new code at specific points |
| Check for errors | `get_diagnostics_for_file` | Shows LSP errors/warnings for a file |
| Safe delete | `safe_delete_symbol` | Checks for references first, fails if not safe |

### Workflow

1. Use `get_symbols_overview` when entering an unfamiliar file
2. Use `find_symbol` to locate specific types/functions/methods
3. Use `find_referencing_symbols` to understand impact before editing
4. Use `rename_symbol` instead of manual find-and-replace
5. Run `get_diagnostics_for_file` after edits to verify correctness
