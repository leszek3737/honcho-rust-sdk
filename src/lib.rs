#![cfg_attr(docsrs, feature(doc_cfg))]
//! # Honcho Rust SDK
//!
//! Rust SDK for [Honcho](https://github.com/plastic-labs/honcho) — AI agent memory
//! and social cognition infrastructure.
//!
//! ## Status
//!
//! **Alpha** — this SDK is under active development and not yet ready for production use.
//!
//! ## Quick start
//!
//! ```no_run
//! # async fn run() -> honcho_ai::Result<()> {
//! use honcho_ai::Honcho;
//!
//! let client = Honcho::new("http://localhost:8000", "my-workspace")?;
//! let alice = client.peer("alice").build().await?;
//! let session = client.session("conversation-1").build().await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Features
//!
//! - **`blocking`** — a synchronous facade over the async API, for callers without a
//!   Tokio runtime. Enables the [`blocking`] module. Off by default.
//! - **`tracing`** — emits [`tracing`](https://docs.rs/tracing) spans and events from the
//!   transport layer for observability. Off by default.

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    missing_docs
)]
#![warn(clippy::pedantic, clippy::cargo, rustdoc::broken_intra_doc_links)]
#![allow(
    clippy::module_name_repetitions,
    clippy::missing_errors_doc,
    clippy::multiple_crate_versions
)]

pub mod client;
pub mod conclusion;
pub mod dialectic_stream;
pub mod error;
// internal transport — no stability guarantees
pub(crate) mod http;
pub mod message;
pub mod peer;
pub mod session;
pub mod types;
pub mod upload;

pub use client::Honcho;
pub use conclusion::{Conclusion, ConclusionCreateParams, ConclusionScope};
pub use dialectic_stream::{DialecticStream, FinalResponse};
pub use message::Message;
pub use peer::Peer;
pub use session::{Session, SessionContextBuilder, UploadFileBuilder};
pub use upload::FileSource;

// Error model, client parameters, and public builders returned by SDK methods.
pub use client::{Environment, HonchoParams};
pub use conclusion::ConclusionRepresentationBuilder;
pub use error::{HonchoError, Result};
pub use peer::{MessageBuilder, RepresentationBuilder};
pub use session::SessionRepresentationBuilder;

pub use types::conclusion::{ConclusionLevel, ConclusionResponse};
pub use types::dialectic::{DialecticOptions, ReasoningLevel};
pub use types::message::{MessageCreate, MessagePage, MessageResponse, MessageSearchOptions};
pub use types::pagination::{Page, PageResponse};
pub use types::peer::{PeerConfig, PeerContext};
pub use types::session::{
    SessionConfiguration, SessionContext, SessionContextOptions, SessionPeerConfig,
    SessionResponse, SessionSummaries,
};
pub use types::workspace::WorkspaceConfiguration;

#[cfg(feature = "blocking")]
#[cfg_attr(docsrs, doc(cfg(feature = "blocking")))]
pub mod blocking;
