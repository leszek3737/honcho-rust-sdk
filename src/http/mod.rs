//! HTTP client internals.
//!
//! These modules are internal implementation details. While they are currently
//! `pub` for technical reasons (see `lib.rs`), they carry **no stability
//! guarantees** — types, functions, and signatures may change in any release.
//! The public API surface is the [`crate::Honcho`], [`crate::Peer`],
//! [`crate::Session`], and related wrapper types, not these transport modules.

/// Low-level HTTP client wrapper around `reqwest`.
pub mod client;
/// Response decoding helpers.
pub mod decode;
/// API route path builders.
pub mod routes;
/// Server-Sent Events (SSE) stream parsing.
pub mod sse;
