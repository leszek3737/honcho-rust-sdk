# AGENTS.md — honcho-ai (Rust SDK)

<!-- gitnexus:start -->
# GitNexus — Code Intelligence

This project is indexed by GitNexus as **honcho-rust-sdk** (2534 symbols, 7911 relationships, 221 execution flows). Use the GitNexus MCP tools to understand code, assess impact, and navigate safely.

> If any GitNexus tool warns the index is stale, run `npx gitnexus analyze` in terminal first.

## Always Do

- **MUST run impact analysis before editing any symbol.** Before modifying a function, class, or method, run `gitnexus_impact({target: "symbolName", direction: "upstream"})` and report the blast radius (direct callers, affected processes, risk level) to the user.
- **MUST run `gitnexus_detect_changes()` before committing** to verify your changes only affect expected symbols and execution flows.
- **MUST warn the user** if impact analysis returns HIGH or CRITICAL risk before proceeding with edits.
- When exploring unfamiliar code, use `gitnexus_query({query: "concept"})` to find execution flows instead of grepping. It returns process-grouped results ranked by relevance.
- When you need full context on a specific symbol — callers, callees, which execution flows it participates in — use `gitnexus_context({name: "symbolName"})`.

## Never Do

- NEVER edit a function, class, or method without first running `gitnexus_impact` on it.
- NEVER ignore HIGH or CRITICAL risk warnings from impact analysis.
- NEVER rename symbols with find-and-replace — use `gitnexus_rename` which understands the call graph.
- NEVER commit changes without running `gitnexus_detect_changes()` to check affected scope.

## Resources

| Resource | Use for |
|----------|---------|
| `gitnexus://repo/honcho-rust-sdk/context` | Codebase overview, check index freshness |
| `gitnexus://repo/honcho-rust-sdk/clusters` | All functional areas |
| `gitnexus://repo/honcho-rust-sdk/processes` | All execution flows |
| `gitnexus://repo/honcho-rust-sdk/process/{name}` | Step-by-step execution trace |

## CLI

| Task | Read this skill file |
|------|---------------------|
| Understand architecture / "How does X work?" | `.claude/skills/gitnexus/gitnexus-exploring/SKILL.md` |
| Blast radius / "What breaks if I change X?" | `.claude/skills/gitnexus/gitnexus-impact-analysis/SKILL.md` |
| Trace bugs / "Why is X failing?" | `.claude/skills/gitnexus/gitnexus-debugging/SKILL.md` |
| Rename / extract / split / refactor | `.claude/skills/gitnexus/gitnexus-refactoring/SKILL.md` |
| Tools, resources, schema reference | `.claude/skills/gitnexus/gitnexus-guide/SKILL.md` |
| Index, status, clean, wiki CLI commands | `.claude/skills/gitnexus/gitnexus-cli/SKILL.md` |

<!-- gitnexus:end -->

## Build & Test Commands

```sh
cargo build
cargo test                              # all tests (unit + integration; int tests skip if no server)
cargo test --lib                        # unit tests only
cargo test --test '*_types'             # schema validation + roundtrip tests (no server needed)
cargo test --test integration           # integration tests (needs `HONCHO_API_URL`)
cargo fmt --check
cargo clippy --all-targets --all-features
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

## Env Var Resolution

`HonchoParams` resolution order: explicit builder arg → env var → default.

| Env var | Field | Default |
|---------|-------|---------|
| `HONCHO_API_KEY` | `api_key` | None |
| `HONCHO_URL` / `HONCHO_API_URL` | `base_url` | `https://api.honcho.dev` |
| `HONCHO_WORKSPACE_ID` | `workspace_id` | `"default"` |

Integration tests use `HONCHO_API_URL` (default `http://localhost:8000`) and `HONCHO_API_KEY`.

## Gotchas

- **Blocking API panics if called inside an async runtime** (`src/blocking/runtime.rs:19`). Users must use the async `Honcho` client instead.
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
