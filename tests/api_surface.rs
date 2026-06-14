#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

//! Public API-surface acceptance tests for the 0.2.0 shape.
//!
//! Almost everything here is **compile-only**: the file is an assertion that
//! the crate's public surface keeps its agreed shape (root re-exports, auto
//! trait bounds, by-value `created_at`, the `PeerSpec: From<Peer>` impls and
//! the `peer`/`session`/`search` builder ergonomics). If any of these regress,
//! the test crate fails to *compile* — the strongest possible guard. The single
//! runnable `#[test]` exists only so the binary reports a passing test.

use static_assertions::assert_impl_all;

// === root re-export presence (compile-only) ===
//
// Isolated in its own module so the `Result` import cannot shadow `std::Result`
// elsewhere in the file. A missing re-export makes this `use` fail to compile.
mod reexport_presence {
    #[allow(unused_imports)]
    use honcho_ai::{
        ConclusionRepresentationBuilder, ConclusionResponse, Environment, HonchoError,
        HonchoParams, MessageBuilder, MessagePage, Page, PageResponse, ReasoningLevel, Result,
        SessionRepresentationBuilder, SessionResponse,
    };
}

// === auto-trait bounds on the error model and core DTOs ===

assert_impl_all!(honcho_ai::HonchoError: std::error::Error, Send, Sync);
assert_impl_all!(honcho_ai::types::message::MessageResponse: Send, Sync);
// A paginated page of DTOs must stay `Send` so it can cross `.await` points.
assert_impl_all!(honcho_ai::Page<honcho_ai::types::message::MessageResponse>: Send);

// === PeerSpec: From<Peer> (and &Peer) impls are present ===
//
// `PeerSpec` is intentionally *not* a crate-root re-export; it lives at
// `honcho_ai::session::PeerSpec`. These bounds compile-assert the conversion
// surface used by `Session::add_peers` / `set_peers`.
const _: fn() = || {
    fn takes<T: From<honcho_ai::Peer>>() {}
    takes::<honcho_ai::session::PeerSpec>();
};
const _: fn() = || {
    fn takes_ref<'a, T: From<&'a honcho_ai::Peer>>() {}
    takes_ref::<honcho_ai::session::PeerSpec>();
};

// === `created_at()` returns `DateTime<Utc>` by value (NOT by reference) ===
//
// The wrapper constructors (`from_raw`/`new`) are `pub(crate)`, so the values
// cannot be built from an external test crate. We instead pin the *method
// signature* via a function-pointer binding: if any `created_at` regressed to
// returning `&DateTime<Utc>`, these coercions would fail to compile.
const _MESSAGE_CREATED_AT: fn(&honcho_ai::Message) -> chrono::DateTime<chrono::Utc> =
    honcho_ai::Message::created_at;
const _CONCLUSION_CREATED_AT: fn(&honcho_ai::Conclusion) -> chrono::DateTime<chrono::Utc> =
    honcho_ai::Conclusion::created_at;
const _SESSION_CREATED_AT: fn(&honcho_ai::Session) -> chrono::DateTime<chrono::Utc> =
    honcho_ai::Session::created_at;

// The blocking facade wrappers mirror the same by-value contract. Gated on the
// `blocking` feature because the types only exist under that cfg; the bindings
// pin `honcho_ai::blocking::{Session,Conclusion}::created_at` to `-> DateTime<Utc>`
// (by value), so a regression to `&DateTime<Utc>` fails to compile.
#[cfg(feature = "blocking")]
const _BLOCKING_SESSION_CREATED_AT: fn(
    &honcho_ai::blocking::Session,
) -> chrono::DateTime<chrono::Utc> = honcho_ai::blocking::Session::created_at;
#[cfg(feature = "blocking")]
const _BLOCKING_CONCLUSION_CREATED_AT: fn(
    &honcho_ai::blocking::Conclusion,
) -> chrono::DateTime<chrono::Utc> = honcho_ai::blocking::Conclusion::created_at;

// === builder ergonomics: peer / session / search ===
//
// Compile-asserts the `client.peer("..").build()` shape (a `bon` builder whose
// finish-fn is `build`). Never executed — calling `.build()` on a `#[builder]`
// async fn returns the future without an async context, so the helper itself is
// a plain (non-async) fn. Each future is bound to a named local (not `let _`,
// which would trip `clippy::let_underscore_future`) and simply dropped at scope
// end; no network I/O happens.
#[allow(dead_code)]
fn _builders_compile(c: &honcho_ai::Honcho) {
    let _peer_fut = c.peer("a").build();
    let _session_fut = c.session("s").build();
    let _search_fut = c.search("q").build();
}

#[test]
fn api_surface_compiles() {
    // All assertions above are compile-time; reaching here means the public
    // surface kept its shape.
}
