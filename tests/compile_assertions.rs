//! Compile-time gate on the public API's auto-trait guarantees.
//!
//! Nothing here runs — these assertions fail to *compile* if a type on the
//! public surface regresses a trait it advertises. `Send + Sync` matter because
//! SDK values cross `.await` points and are driven from multi-threaded Tokio
//! runtimes; `Send + 'static` additionally gates the futures users hand to
//! `tokio::spawn`. An accidental `Rc`/`RefCell` in a field, a dropped `Clone`,
//! or a stray `!Send` inner type is caught here at build time, not in production.

#![allow(clippy::used_underscore_items)]

use std::fmt::Display;

use honcho_ai::types::conclusion::ConclusionResponse;
use honcho_ai::types::dialectic::{DialecticOptions, ReasoningLevel};
use honcho_ai::types::message::{
    MessageCreate, MessagePage, MessageResponse, MessageSearchOptions,
};
use honcho_ai::types::pagination::{Page, PageResponse};
use honcho_ai::types::peer::Peer as PeerResponse;
use honcho_ai::types::peer::{PeerConfig, PeerContext};
use honcho_ai::types::session::{
    SessionConfiguration, SessionContext, SessionContextOptions, SessionPeerConfig,
    SessionResponse, SessionSummaries,
};
use honcho_ai::types::workspace::WorkspaceConfiguration;
use honcho_ai::{
    Conclusion, ConclusionCreateParams, ConclusionRepresentationBuilder, ConclusionScope,
    DialecticStream, Environment, FileSource, FinalResponse, Honcho, HonchoParams, Message,
    MessageBuilder, Peer, RepresentationBuilder, Session, SessionRepresentationBuilder,
};
use static_assertions::{assert_impl_all, assert_not_impl_any};

// --- Core client handles: cheap-clone, thread-safe ---
assert_impl_all!(Honcho: Send, Sync, Clone);
// `Peer` was previously asserted twice via inconsistent paths (`honcho_ai::Peer`
// for `Debug`, `Peer` for the auto-traits); merged into one statement.
assert_impl_all!(Peer: Send, Sync, Clone, std::fmt::Debug);
assert_impl_all!(Session: Send, Sync, Clone);

// --- Domain values ---
assert_impl_all!(Conclusion: Send, Sync, Clone, std::fmt::Debug, Display);
assert_impl_all!(ConclusionScope: Send, Sync, Clone);
assert_impl_all!(Message: Send, Sync, Clone, std::fmt::Debug, Display);
assert_impl_all!(honcho_ai::error::HonchoError: Send, Sync, std::error::Error);

// --- Dialectic streaming ---
// `DialecticStream<S>` derives its auto-traits from `S`, so the concrete
// monomorphization the SDK hands back must stay `Send + Sync` to be driven from
// a multi-threaded runtime. The inner `Empty<Result<String>>` is `Send + Sync`
// (it relies on `HonchoError: Send + Sync`, asserted above).
assert_impl_all!(
    DialecticStream<futures_util::stream::Empty<honcho_ai::error::Result<String>>>:
        Send, Sync, std::fmt::Debug
);

// --- Export-coverage: the remaining public surface ---
assert_impl_all!(FinalResponse: Send, Sync, Clone, std::fmt::Debug, Display);
assert_impl_all!(ConclusionCreateParams: Send, Sync, Clone, std::fmt::Debug);
assert_impl_all!(DialecticOptions: Send, Sync, Clone, std::fmt::Debug);
assert_impl_all!(ReasoningLevel: Send, Sync, Clone, Copy, std::fmt::Debug);
assert_impl_all!(MessageCreate: Send, Sync, Clone, std::fmt::Debug);
assert_impl_all!(MessageResponse: Send, Sync, Clone, std::fmt::Debug);
assert_impl_all!(MessageSearchOptions: Send, Sync, Clone, std::fmt::Debug);
assert_impl_all!(MessagePage: Send, Sync, Clone);
assert_impl_all!(PeerConfig: Send, Sync, Clone, std::fmt::Debug);
assert_impl_all!(PeerContext: Send, Sync, Clone, std::fmt::Debug);
assert_impl_all!(SessionConfiguration: Send, Sync, Clone, std::fmt::Debug);
assert_impl_all!(SessionContext: Send, Sync, Clone, std::fmt::Debug);
assert_impl_all!(SessionContextOptions: Send, Sync, Clone, std::fmt::Debug);
assert_impl_all!(SessionPeerConfig: Send, Sync, Clone, std::fmt::Debug);
assert_impl_all!(SessionResponse: Send, Sync, Clone, std::fmt::Debug);
assert_impl_all!(SessionSummaries: Send, Sync, Clone, std::fmt::Debug);
assert_impl_all!(WorkspaceConfiguration: Send, Sync, Clone, std::fmt::Debug);
assert_impl_all!(ConclusionResponse: Send, Sync, Clone, std::fmt::Debug);

// Client construction surface.
assert_impl_all!(Environment: Send, Sync, Clone, Copy, std::fmt::Debug);
assert_impl_all!(HonchoParams: Send, Sync);

// `Page<TRaw, TOut>` is generic; its auto-traits derive from the inner item
// type and its (boxed) transform/fetcher closures. The SDK only ever hands back
// `Page` over `Send + Sync` items, so assert the concrete monomorphization
// returned by the list endpoints stays thread-safe + cheap-clone (Arc bump).
// `PageResponse<T>` is the plain serde-facing struct.
assert_impl_all!(Page<PeerResponse>: Send, Sync, Clone, std::fmt::Debug);
assert_impl_all!(PageResponse<PeerResponse>: Send, Sync, Clone, std::fmt::Debug);

// Public builders returned from SDK methods. These own an (Arc-backed)
// `HttpClient` plus owned `String`/`Option` fields — no `Rc`/`RefCell`/borrow —
// so they stay `Send + Sync`. None derive `Clone` (each is consumed by chained
// `mut self -> Self` setters).
assert_impl_all!(MessageBuilder: Send, Sync);
assert_impl_all!(RepresentationBuilder: Send, Sync);
assert_impl_all!(SessionRepresentationBuilder: Send, Sync);
assert_impl_all!(ConclusionRepresentationBuilder: Send, Sync);

// Public builders returned from SDK methods. `SessionContextBuilder` owns an
// (Arc-backed) `HttpClient`, so it stays `Send + Sync` (it is not `Clone` — it
// is consumed by chained `mut self -> Self` setters). `UploadFileBuilder`
// borrows its `Session` and carries a `FileSource`, so it is `Send` but neither
// `Sync` nor `Clone`.
assert_impl_all!(honcho_ai::SessionContextBuilder: Send, Sync);
assert_impl_all!(honcho_ai::UploadFileBuilder<'_>: Send);
// `UploadFileBuilder` carries a `FileSource` (boxed `dyn AsyncRead + Send`),
// which is neither `Sync` nor `Clone`, so the builder must not advertise them
// either — guards against a future field swap silently widening the bound.
assert_not_impl_any!(honcho_ai::UploadFileBuilder<'_>: Sync, Clone, Copy);

// `FileSource` is intentionally `Send` (uploads cross `.await`) but not `Sync`
// or `Clone`: the `Stream` variant boxes a `dyn AsyncRead + Send` reader, which
// is neither `Sync` nor `Clone`.
assert_impl_all!(FileSource: Send, std::fmt::Debug);
assert_not_impl_any!(FileSource: Sync, Clone, Copy);

// Auto-trait propagation must be *conditional*: a `DialecticStream` wrapping a
// non-`Send` inner stream must itself be non-`Send`/non-`Sync`. This guards
// against someone "fixing" a Send error with an unconditional `unsafe impl`.
assert_not_impl_any!(
    DialecticStream<futures_util::stream::Empty<std::rc::Rc<()>>>: Send, Sync
);

// --- Borrowed futures must be `Send` (driveable on a multi-threaded runtime) ---
fn _assert_future_send<F: std::future::Future + Send>(_: F) {}

// --- Futures over owned receivers must be `Send + 'static` (spawn-ready) ---
fn _assert_future_send_static<F: std::future::Future + Send + 'static>(_: F) {}

fn _honcho_peers_future_is_send(h: &Honcho) {
    _assert_future_send(h.peers());
}

fn _honcho_search_future_is_send(h: &Honcho) {
    _assert_future_send(h.search("q").build());
}

fn _peer_chat_stream_future_is_send(p: &Peer) {
    // No `async move` wrapper: the builder owns its data, so `send()` already
    // returns the future directly.
    _assert_future_send(p.chat_stream("q").send());
}

fn _session_messages_future_is_send(s: &Session) {
    _assert_future_send(s.messages());
}

// Spawn-readiness: with an *owned* receiver the future captures no external
// borrow, so it is `'static` and can be handed to `tokio::spawn`.
fn _honcho_peers_future_is_spawnable(h: Honcho) {
    _assert_future_send_static(async move {
        let _ = h.peers().await;
    });
}

fn _honcho_search_future_is_spawnable(h: Honcho) {
    _assert_future_send_static(async move {
        let _ = h.search("q").build().await;
    });
}

fn _peer_chat_stream_future_is_spawnable(p: Peer) {
    _assert_future_send_static(async move {
        let _ = p.chat_stream("q").send().await;
    });
}

fn _session_messages_future_is_spawnable(s: Session) {
    _assert_future_send_static(async move {
        let _ = s.messages().await;
    });
}

// --- Blocking facade (feature = "blocking") ---
// Each blocking handle wraps the async client behind an internal runtime and
// must stay cheap-clone + thread-safe.
#[cfg(feature = "blocking")]
mod blocking_assertions {
    use honcho_ai::blocking::{
        BlockingSessionContextBuilder, BlockingSessionRepresentationBuilder,
        BlockingUploadFileBuilder, ChatStreamIterator, Conclusion, ConclusionScope, Honcho, Peer,
        Session,
    };
    use static_assertions::{assert_impl_all, assert_not_impl_any};

    // Core blocking handles: cheap-clone + thread-safe.
    assert_impl_all!(Honcho: Send, Sync, Clone);
    assert_impl_all!(Peer: Send, Sync, Clone);
    assert_impl_all!(Session: Send, Sync, Clone);

    // Blocking domain values wrap their (Arc-backed) async counterparts and
    // derive `Clone`, so they stay `Send + Sync + Clone`.
    assert_impl_all!(Conclusion: Send, Sync, Clone, std::fmt::Debug, std::fmt::Display);
    assert_impl_all!(ConclusionScope: Send, Sync, Clone, std::fmt::Debug);

    // The streaming iterator is built on one thread and driven from another by
    // the blocking runtime, so it must be `Send` (it is not `Sync`/`Clone`).
    assert_impl_all!(ChatStreamIterator: Send);
    assert_not_impl_any!(ChatStreamIterator: Clone, Copy);

    // Blocking builders wrap the async builders behind the internal runtime and
    // stay `Send + Sync` (consumed by chained `mut self -> Self` setters, so not
    // `Clone`). `BlockingUploadFileBuilder` additionally owns an async
    // `UploadFileBuilder` carrying a non-`Sync` `FileSource`, so it is `Send`
    // only.
    assert_impl_all!(BlockingSessionContextBuilder: Send, Sync, std::fmt::Debug);
    assert_impl_all!(BlockingSessionRepresentationBuilder: Send, Sync, std::fmt::Debug);
    assert_impl_all!(BlockingUploadFileBuilder<'_>: Send, std::fmt::Debug);
    assert_not_impl_any!(BlockingUploadFileBuilder<'_>: Sync, Clone, Copy);
}
