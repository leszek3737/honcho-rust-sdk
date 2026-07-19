# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Port of three upstream (Python/TypeScript SDK, commit `14538cfc`) features into
the Rust SDK.

### Added

- **`ConclusionLevel` enum + `level` field on `ConclusionResponse`.** Mirrors
  the upstream `Literal["explicit", "deductive", "inductive", "contradiction"]`.
  `"explicit"` conclusions are extracted directly from messages; the other
  variants are derived during dreaming. Surfaced as `Conclusion::level()` and
  the blocking mirror, re-exported from the crate root as
  `honcho_ai::ConclusionLevel`. Defaults to `Explicit` when the server omits
  the field (`#[serde(default)]`).

- **Generic free-form `filters` on `ConclusionScope::list()` / `query()`.**
  Both builders gain a `.filters(HashMap<String, serde_json::Value>)` method
  that lets callers add arbitrary filter criteria (e.g. `{"level": "explicit"}`
  to retrieve only non-dream-derived conclusions). Brings `conclusion.rs` into
  line with the `Option<HashMap<String, serde_json::Value>>` pattern already
  used by peer / session / message / workspace filters.

### Changed

- **`ConclusionGet::filters` and `ConclusionQuery::filters` are now
  `Option<HashMap<String, serde_json::Value>>`** (matching the free-form
  `additionalProperties: true` shape in the OpenAPI spec), replacing the
  bespoke typed `ConclusionFilters` struct that only modeled
  `observer_id`/`observed_id`/`session_id`. `ConclusionFilters` is **removed**;
  the bespoke type was an anomaly — every other list/query type in the SDK
  already used the generic map. Source-compatible for callers that constructed
  filters via the builders; a small mechanical migration at the raw-DTO level
  (the field type changed).

- **`ConclusionGet` drops its `Eq` derive** (keeps `PartialEq`): the new
  `HashMap<String, serde_json::Value>` element type contains `serde_json::Value`,
  which is not `Eq` (it can carry `f64` with `NaN`).

### Added (Guards)

- **Reserved-key guard on `list()` / `query()` filters.** Passing the
  scope-managed keys (`observer`, `observed`, `observer_id`, `observed_id`,
  `session`, `session_id`) to `ListConclusionsBuilder::filters` — or the
  peer-pair keys to `QueryConclusionsBuilder::filters` — now returns
  `HonchoError::Validation` with a clear, machine-readable message pointing at
  the supported API surface (`.session()` for `list`; pick the peer pair via
  `peer.conclusions()` / `peer.conclusions_of(target)`). `query()` does not
  reject `session` / `session_id` because it has no `.session()` method, so
  `session_id` is a legitimate caller-supplied filter there.

## [0.2.0] - 2026-06-14

This release tightens the public API surface ahead of `1.0`. It contains
several **breaking changes**; please read the **Migration guide** below before
upgrading. Most breaks are mechanical one-line fixes.

### BREAKING

> ⚠️ **Silent behavioral change — audit your error matching.** The
> `HonchoError::Serialization` recategorization (see the variant entry below) is
> the only break in this release that does **not** produce a compile error. Code
> with a wildcard (`_ => …`) arm in a `match` over `HonchoError` keeps compiling
> unchanged, yet its runtime behavior changes: serde **serialize** failures that
> previously surfaced as `HonchoError::Configuration` / `HonchoError::Decode` now
> surface as `HonchoError::Serialization`. **Action:** audit every `match` /
> `if let` that recovers from serialize failures via `HonchoError::Configuration`
> or `HonchoError::Decode` and move that handling to `HonchoError::Serialization`.
> Every *other* break below is a compile error the compiler points you to — this
> one is not, so it is easy to miss.

- **`http` module is now private.** `pub mod http` became `pub(crate)`. The
  module never carried any stability guarantees. Stop importing
  `honcho_ai::http::*`; use the public client API instead.
- **DTO renames.** `honcho_ai::types::conclusion::Conclusion` →
  `ConclusionResponse`, and `honcho_ai::types::session::Session` →
  `SessionResponse`. The high-level wrapper types `honcho_ai::Conclusion` and
  `honcho_ai::Session` are **unchanged** — only the raw DTOs moved.
- **`created_at()` returns by value.** `created_at()` on `Message`, `Session`,
  and `Conclusion` — plus the blocking `Conclusion` mirror — now returns
  `DateTime<Utc>` by value instead of `&DateTime<Utc>`. `DateTime<Utc>` is
  `Copy`, so drop the leading `*`/`&` at call sites. The blocking `Session`
  wrapper had no `created_at()` accessor before; its newly added getter returns
  by value from the start (additive, not a changed signature).
- **Builder-based client entry points.** `Honcho::peer`, `Honcho::session`, and
  `Honcho::search` (plus blocking mirrors) replace their positional `Option`
  arguments with `bon` builders. `client.peer("a", None, None)` becomes
  `client.peer("a").build()` (or `.config(cfg).build()`). The default `limit`
  for `search` is **unchanged** (it was already `10` via `limit.unwrap_or(10)`);
  it is now expressed declaratively as `#[builder(default = 10)]`.
- **`#[non_exhaustive]` on public DTOs/enums.** Added to `FileSource`,
  `Environment`, `MessageCreate`, `MessageUpdate`, `MessageSearchOptions` (and
  other public DTOs/enums). Out-of-crate struct literals and exhaustive `match`
  arms no longer compile — construct values via their builder or `Default`, and
  add a `_ => …` wildcard arm to matches over these enums.
- **New `HonchoError::Serialization` variant (silent behavioral break — see the
  warning at the top of this section).** serde **serialize** failures are
  recategorized away from `Configuration`/`Decode` into the new `Serialization`
  variant. Code that matched `Configuration`/`Decode` to handle serialize
  failures must now match `Serialization`. `HonchoError` is `#[non_exhaustive]`,
  so an existing wildcard arm already absorbs the new variant — which is exactly
  why this break is **behavioral, not compile-time**: downstream still compiles,
  but error-matching behavior shifts. Audit your error handling rather than
  relying on the compiler to flag it.
- **Blocking filtered listings are now paginated.** Blocking
  `Honcho::peers_with_filters` and `Honcho::sessions_with_filters` now return
  the paginated `Page<…>` shape (parity with the async API) instead of only the
  first page.

### Added

- Root re-exports for ergonomic imports: `HonchoError`, `Result`,
  `HonchoParams`, `Environment`, `ReasoningLevel`, `Page`, `PageResponse`,
  `MessagePage`, the builder types, and the `*Response` DTOs
  (`ConclusionResponse`, `SessionResponse`, …).
- `impl From<Peer> for PeerSpec` and
  `impl From<(&Peer, SessionPeerConfig)> for PeerSpec` — an owned `Peer` no
  longer needs to be borrowed (`&`) when building a `PeerSpec`.
- docs.rs feature badges so feature-gated items are clearly marked in the
  rendered documentation.

### Deferred / known SemVer debt

- **Validated newtypes are intentionally deferred.** Wrapping `PeerId`,
  `WorkspaceId`, and `SessionId`, plus using `NonZeroU32` for `top_k`/`limit`,
  was deliberately left out of this release. Introducing these types later is
  itself a breaking change, so this debt should be revisited and resolved
  **before `1.0`**.

### Migration guide

Apply these one-line fixes, in order:

1. **`http` imports** — remove any `use honcho_ai::http::*;` (and similar) and
   switch to the public client API.
2. **DTO renames** — replace `types::conclusion::Conclusion` with
   `ConclusionResponse` and `types::session::Session` with `SessionResponse`.
   Leave `honcho_ai::Conclusion` / `honcho_ai::Session` as-is.
3. **`created_at()`** — drop the leading `*`/`&`: `let ts = msg.created_at();`.
4. **Client entry points** — convert positional calls to builders:
   `client.peer("a", None, None)` → `client.peer("a").build()`; pass config via
   `.config(cfg).build()`. `search`'s default `limit` is unchanged (was already
   `10`), now expressed via `#[builder(default = 10)]` — no behavior change.
5. **`#[non_exhaustive]` DTOs/enums** — replace struct literals with the
   builder/`Default`, and add a `_ => …` arm to any exhaustive `match`.
6. **`HonchoError`** — for serde serialize failures, match the new
   `Serialization` variant instead of `Configuration`/`Decode` (a wildcard arm
   keeps compiling regardless).
7. **Blocking filtered listings** — handle the `Page<…>` return of
   `peers_with_filters` / `sessions_with_filters` (iterate/collect pages) rather
   than assuming first-page-only results.

## [0.1.6] - 2026-06-14

A non-breaking hardening release. No API changes — a series of correctness,
security, and robustness fixes across the transport, error, and domain layers
(PRs #7–#11).

### Fixed

- **HTTP transport** — hardened the transport layer for security and
  correctness.
- **Error model** — hardened the error model and closed retry-semantics test
  gaps.
- **Blocking API** — eliminated panics and silent corruption in the sync
  (blocking) API.
- **Domain types** — improved correctness and consistency across the core
  domain types.
- **Types / serde** — serde forward-compatibility and validation hardening.

## [0.1.5] - 2026-05-28

### Added

- `ConclusionFilters` typed struct for conclusion list and query requests (replaces raw `JsonValue` filters).
- `SessionCreate::validate()` for client-side session ID validation (non-empty, `[a-zA-Z0-9_-]` only).
- `validate_search_params()` shared validation for `search_top_k`, `search_max_distance`, and `max_conclusions` range checks.
- `serialize_upload_fields()` reusable helper for multipart upload form field serialization.
- `SessionContext::build_context_messages()` and `format_peer_card()` shared helpers for OpenAI/Anthropic context formatting.
- `BlockingUploadFileBuilder::with_inner()` internal helper reducing boilerplate in blocking upload builder methods.
- `push_param!` macro for building query parameter lists in `ContextBuilder::send()`.
- Documentation for `DialecticOptions::validate()` with usage example.

### Changed

- `Peer::refresh()` and `Peer::get_configuration_raw()` now share `fetch_and_update_cache()` helper.
- `Peer::context()` delegates to `ContextBuilder` instead of building query parameters manually.
- `RepresentationBuilder::send()` and `ContextBuilder::send()` use shared `validate_search_params()`.
- `map_to_peer_config()` avoids intermediate `serde_json::to_value` serialization — maps directly to `Value::Object`.
- Conclusion list and query requests use typed `ConclusionGet`/`ConclusionQuery` with `ConclusionFilters` instead of `serde_json::json!`.
- `BlockingUploadFileBuilder` methods delegate through `with_inner()` instead of manual struct construction.
- `to_openai()` and `to_anthropic()` use shared `build_context_messages()` loop instead of duplicated inline formatting.
- `validate_pagination()` uses `.into()` instead of `.to_string()` for error messages.

## [0.1.4] - 2026-05-27

### Added

- `SessionContextBuilder` (async) and `BlockingSessionContextBuilder` (blocking) to provide a builder pattern for fine-grained session context queries.
- Extensive client-side validation for session context builders (including range checks, `tokens > 0`, and cross-parameter validation).

### Changed

- `Session::context()` now utilizes `SessionContextBuilder` under the hood.

### Fixed

- Added missing validation for `tokens > 0` on `SessionContextOptions` and `SessionContextBuilder`.

## [0.1.2] - 2026-05-25

### Added

- `HonchoError::is_retryable()` to expose the SDK retry policy for callers.
- Client-side validation for dialectic queries, including the 10,000 character maximum.
- Client-side validation for pagination parameters (`page >= 1`, `size` between 1 and 100).
- Client-side validation for workspace IDs (`1..=512`, ASCII alphanumeric, `_`, or `-`).
- CI MSRV check for Rust 1.88.0.

### Changed

- Workspace metadata and configuration reads now use the OpenAPI `POST /v3/workspaces` get-or-create endpoint.
- `Honcho::base_url()` now reports the same normalized base URL used by the HTTP client.
- CI now runs the full all-features test suite and avoids duplicate doctest execution.
- Upload and peer-configuration docs now describe supported MIME types and peer existence requirements.

### Fixed

- Invalid base URLs such as `localhost:8000`, unsupported schemes, or URLs without hosts are rejected during client construction.
- Base URLs with non-root trailing slashes are normalized consistently between `Honcho` and `HttpClient`.
- `Honcho::schedule_dream` rejects an empty `observer` before making network requests.
- Pagination rejects invalid `page`/`size` values before making network requests.
- Route and docs tests cover new validation boundaries and workspace get-or-create behavior.
- README upload and peer-management examples corrected.

## [0.1.1] - 2025-05-13

### Breaking Changes

- **R-03**: `Session::context_with_options` now takes `&SessionContextOptions` instead of `(bool, bool)`. Use the builder pattern for options.
- **R-07**: `Page::next_page` now returns `Result<Option<Page<T>>>` instead of `Option<Page<T>>`. HTTP errors propagate as `Err` instead of being silently swallowed as `None`.
- **R-08**: `collect_all_pages` now returns `Result<Vec<T>>` instead of `Vec<T>`. Pagination errors are no longer silently dropped.
- **R-20**: Session message methods (`add_messages`, `get_message`, `update_message`, `search`, `search_with_options`) now return `Message` (accessor methods) instead of `MessageResponse` (direct field access). Blocking equivalents also updated.
- **R-22**: All `bon::Builder` structs use `finish_fn = build`. No migration needed if already calling `.build()`.
- **R-27**: `DialecticStream::final_response` returns `FinalResponse` struct (`.content` field) instead of `&str`.

### Added

- `Conclusion` wrapper with accessor methods and custom `Debug`/`Display`
- `ConclusionScope` for self-scoped and cross-peer conclusion access
- `ConclusionScope::create`, `create_batch`, `list`, `get`, `delete`, `query` (semantic search), `representation`
- `Peer::conclusions()` and `Peer::conclusions_of(target)` for scoped access
- `Peer::representation_builder()` with fine-grained parameters (search_query, search_top_k, search_max_distance, include_most_frequent, max_conclusions)
- `Peer::chat_with_options` for full dialectic options (session, target, reasoning level)
- `Peer::chat_stream` builder with `.target()`, `.session()`, `.reasoning_level()` chainable methods
- `Peer::context_with_target` for scoped context retrieval
- `Peer::get_card`, `get_card_with_target`, `set_card`, `set_card_with_target` for peer card CRUD
- `Peer::sessions` and `sessions_with_options` for paginated session listing
- `Peer::search` and `search_with_options` for message search
- `Peer::update` for patch-style metadata updates
- `Session::peers()` returning `Vec<Peer>` wrappers
- `Session::add_peer`, `add_peers`, `set_peers`, `remove_peers` with `PeerSpec` enum (bare ID or with config)
- `Session::get_peer_configuration`, `set_peer_configuration` for per-peer session config
- `Session::upload_file` and `upload_file_streamed` with builder pattern (`.peer()`, `.metadata()`, `.configuration()`, `.created_at()`)
- `Session::clone_session`, `clone_session_with_message`
- `Session::representation(peer_id)` scoped to session
- `Session::queue_status()` for processing status
- `Session::summaries()` for short/long summary retrieval
- `Session::messages()` paginated message listing
- `Session::delete()` for session removal
- `Honcho::force_ensure()` for explicit workspace creation
- `Honcho::schedule_dream(observer)` for memory consolidation
- `Honcho::search` for workspace-wide message search
- `Honcho::queue_status` for workspace processing status
- `Honcho::peers`, `peers_with_filters`, `sessions`, `sessions_with_filters`, `workspaces` for paginated listing
- `Honcho::get_configuration`, `set_configuration` for typed workspace config
- `Honcho::get_configuration_raw`, `set_configuration_raw` for raw JSON config access
- `Honcho::delete_workspace` for workspace removal
- `Message` wrapper type with `id()`, `content()`, `peer_id()`, `session_id()`, `metadata()`, `created_at()`, `token_count()`, `workspace_id()` accessors and `Display` impl
- `FileSource` enum for file uploads (`bytes`, `path`, `stream`)
- `DialecticStream` adapter that accumulates SSE content and provides `final_response()` / `is_complete()`
- `blocking` feature: sync wrappers over the full async API with runtime guard
- `tracing` feature: `#[tracing::instrument]` on all public async methods
- `SessionContext::to_openai` and `to_anthropic` for provider-compatible message format conversion
- `Page::into_stream` for auto-fetching paginated stream
- `Page::map` for item transform chaining
- Options types: `SessionListOptions`, `DialecticOptions`, `MessageSearchOptions` with builder pattern

### Changed

- MSRV raised to 1.88
- All `bon::Builder` structs use `finish_fn = build` for consistency
- Error type provides `code()` method returning machine-readable string identifiers

### Fixed

- Duplicate `Page::next_page` section in MIGRATION.md removed
- SSE stream handles UTF-8 splits at chunk boundaries
- Cancel-safety for SSE stream drops

### Known Limitations

- No webhooks or API keys endpoints
- No automatic SSE reconnection
- MSRV 1.88

## [0.1.0] - 2025-05-13

### Added

- Error types with HTTP status mapping (5xx → Server, 429 → RateLimit, etc.) and Retry-After parsing
- 55+ request/response type schemas with OpenAPI validation and serde roundtrip tests
- HTTP client with automatic retries, exponential backoff, and configurable max retries
- Paginated collection streaming (Page → Stream → Iterator)
- Honcho client: workspace auto-creation, metadata/configuration CRUD
- Peer: chat (dialectic), streaming chat, representation, context, card, conclusions
- Session: peer management, messages (batch up to 100), file upload (multipart streaming), clone, summaries
- SSE streaming with cancel-safety and UTF-8 split handling
- Conclusion & ConclusionScope: create, list, query (semantic search), representation, delete
- Blocking facade (feature-gated): sync wrappers over async API with runtime guard
- Compile-time assertions: Send + Sync + 'static bounds on all public types
- Session context parity with Python SDK (to_openai / to_anthropic)
- CI workflow: fmt, clippy, test, doc, MSRV verify
- Integration tests with graceful skip when no server available
