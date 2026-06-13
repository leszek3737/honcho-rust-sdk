//! Blocking facade over the async [`Honcho`][crate::Honcho] SDK.
//!
//! Enable with the `blocking` feature flag. Each call drives the underlying
//! async client on a lazily-created internal tokio multi-thread runtime, so
//! users do not need their own runtime.
//!
//! # Errors
//!
//! This facade never panics on its own. If a blocking call is made from inside
//! an active async runtime, it returns
//! [`HonchoError::Configuration`](crate::error::HonchoError) instead. The canonical
//! message is: "blocking API cannot be used from inside an async runtime; use
//! the async Honcho client instead." Use the async [`Honcho`][crate::Honcho]
//! client in that context.
//!
//! # Example
//!
//! ```no_run
//! use honcho_ai::blocking::Honcho;
//!
//! let client = Honcho::new("https://api.honcho.dev", "default")?;
//! let peers = client.peers()?;
//! println!("{} peers", peers.len());
//! # Ok::<(), honcho_ai::error::HonchoError>(())
//! ```
//!
//! # Unnameable builder types (interim)
//!
//! Six public builder types are returned from public methods but are currently
//! **not re-exported** from this module, so they cannot be named in user code
//! (e.g. as a struct field, a function return type, or a rustdoc link). They
//! can still be obtained via the corresponding methods and driven with chained
//! `.build()` calls:
//!
//! | Method | Returned (unnameable) builder |
//! |--------|-------------------------------|
//! | `Peer::chat_stream` | `BlockingChatStreamBuilder` |
//! | `Peer::representation_builder` | `BlockingRepresentationBuilder` |
//! | `Peer::context_builder` | `BlockingContextBuilder` |
//! | `ConclusionScope::representation` | `BlockingConclusionRepresentationBuilder` |
//! | `ConclusionScope::list` | `BlockingListConclusionsBuilder` |
//! | `ConclusionScope::query` | `BlockingQueryConclusionsBuilder` |
//!
//! The re-exports (plus `#![warn(unnameable_types)]` in the crate root) are a
//! breaking change to the public surface and are deferred to a future release
//! (tracked for PR6). Until then, store the produced builder inline or factor
//! it through a generic helper rather than naming the type.

mod client;
mod conclusion;
mod iter;
mod peer;
mod runtime;
mod session;

pub use client::Honcho;
pub use conclusion::{Conclusion, ConclusionScope};
pub use peer::{ChatStreamIterator, Peer};
pub use session::{
    BlockingSessionContextBuilder, BlockingSessionRepresentationBuilder, BlockingUploadFileBuilder,
    Session,
};
