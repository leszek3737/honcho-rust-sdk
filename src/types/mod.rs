//! Shared types for the Honcho SDK.

pub mod common;
pub mod conclusion;
pub mod dialectic;
pub mod dream;
pub mod message;
pub mod pagination;
pub mod peer;
pub mod session;
pub mod validation;
pub mod workspace;

// Re-export the key DTOs and generic helpers at the `types` root so callers can
// reach them without naming the defining submodule. Convention: one `pub use`
// per submodule, braces only when re-exporting more than one symbol (mirrors the
// crate-root convention in `lib.rs`). Additive — the canonical submodule paths
// (e.g. `types::message::MessageResponse`) remain valid.
pub use conclusion::ConclusionResponse;
pub use message::MessageResponse;
pub use pagination::{Page, PageResponse};
pub use session::SessionResponse;
