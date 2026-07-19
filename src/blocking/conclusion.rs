use std::collections::HashMap;

use crate::conclusion::{ConclusionCreateParams, ConclusionScope as AsyncConclusionScope};
use crate::error::Result;
use crate::types::conclusion::{ConclusionLevel, ConclusionPage};

use super::runtime::block_on;

// NOTE (deferred intentionally to a future major release):
// * `Conclusion` here is a pure data wrapper that duplicates 7 getters plus
//   `Debug`/`Display` of `crate::Conclusion`. Once a breaking release is
//   acceptable, replace it with `pub use crate::Conclusion;`.
// * The `Blocking*` builder prefixes below are redundant inside the
//   `blocking::` module (`crate::ConclusionScope` already disambiguates), but
//   renaming them alters public paths and is therefore deferred to a breaking
//   release.

/// Synchronous wrapper around [`crate::Conclusion`].
#[derive(Clone)]
pub struct Conclusion {
    inner: crate::Conclusion,
}

impl Conclusion {
    pub(crate) fn new(inner: crate::Conclusion) -> Self {
        Self { inner }
    }

    /// Unique identifier.
    #[must_use]
    pub fn id(&self) -> &str {
        self.inner.id()
    }

    /// Content text.
    #[must_use]
    pub fn content(&self) -> &str {
        self.inner.content()
    }

    /// Observer peer ID.
    #[must_use]
    pub fn observer_id(&self) -> &str {
        self.inner.observer_id()
    }

    /// Observed peer ID.
    #[must_use]
    pub fn observed_id(&self) -> &str {
        self.inner.observed_id()
    }

    /// Optional session scope.
    #[must_use]
    pub fn session_id(&self) -> Option<&str> {
        self.inner.session_id()
    }

    /// Creation timestamp.
    #[must_use]
    pub fn created_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.inner.created_at()
    }

    /// Reasoning level. See [`crate::Conclusion::level`].
    #[must_use]
    pub fn level(&self) -> ConclusionLevel {
        self.inner.level()
    }

    /// Workspace ID.
    #[must_use]
    pub fn workspace_id(&self) -> &str {
        self.inner.workspace_id()
    }
}

impl std::fmt::Debug for Conclusion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inner.fmt(f)
    }
}

impl std::fmt::Display for Conclusion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inner.fmt(f)
    }
}

/// Synchronous wrapper around [`crate::ConclusionScope`].
#[derive(Clone)]
pub struct ConclusionScope {
    inner: AsyncConclusionScope,
}

impl std::fmt::Debug for ConclusionScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConclusionScope")
            .field("observer_id", &self.inner.observer_id())
            .field("observed_id", &self.inner.observed_id())
            .finish()
    }
}

impl ConclusionScope {
    pub(crate) fn new(inner: AsyncConclusionScope) -> Self {
        Self { inner }
    }

    /// Observer peer ID.
    #[must_use]
    pub fn observer_id(&self) -> &str {
        self.inner.observer_id()
    }

    /// Observed peer ID.
    #[must_use]
    pub fn observed_id(&self) -> &str {
        self.inner.observed_id()
    }

    /// Create one or more conclusions.
    ///
    /// # Errors
    ///
    /// Returns an [`HonchoError`](crate::error::HonchoError) variant if the
    /// underlying HTTP request fails (network, authentication, validation, or
    /// server-side errors).
    ///
    /// Also returns
    /// [`HonchoError::Configuration`](crate::error::HonchoError::Configuration)
    /// when invoked from inside a tokio/async runtime; in that case use the
    /// async [`crate::ConclusionScope`] instead.
    pub fn create(
        &self,
        conclusions: impl IntoIterator<Item = impl Into<ConclusionCreateParams>>,
    ) -> Result<Vec<Conclusion>> {
        block_on(self.inner.create(conclusions))?
            .map(|v| v.into_iter().map(Conclusion::new).collect())
    }

    /// Builder for scoped representation.
    #[must_use]
    pub fn representation(&self) -> BlockingConclusionRepresentationBuilder {
        BlockingConclusionRepresentationBuilder {
            inner: self.inner.representation(),
        }
    }

    /// Builder for listing conclusions.
    #[must_use]
    pub fn list(&self) -> BlockingListConclusionsBuilder {
        BlockingListConclusionsBuilder {
            inner: self.inner.list(),
        }
    }

    /// Builder for querying conclusions.
    #[must_use]
    pub fn query(&self, query: impl Into<String>) -> BlockingQueryConclusionsBuilder {
        BlockingQueryConclusionsBuilder {
            inner: self.inner.query(query),
        }
    }

    /// Delete a conclusion by ID.
    ///
    /// # Errors
    ///
    /// Returns an [`HonchoError`](crate::error::HonchoError) variant if the
    /// underlying HTTP request fails (network, authentication, validation, or
    /// server-side errors).
    ///
    /// Also returns
    /// [`HonchoError::Configuration`](crate::error::HonchoError::Configuration)
    /// when invoked from inside a tokio/async runtime; in that case use the
    /// async [`crate::ConclusionScope`] instead.
    pub fn delete(&self, conclusion_id: impl Into<String>) -> Result<()> {
        block_on(self.inner.delete(conclusion_id))?
    }
}

/// Blocking builder for scoped representation.
///
/// Not `Clone`: the wrapped async
/// [`crate::conclusion::ConclusionRepresentationBuilder`] is itself not `Clone`,
/// so deriving `Clone` here would require widening the change beyond this
/// module. Rebuild from [`ConclusionScope::representation`] for retries.
pub struct BlockingConclusionRepresentationBuilder {
    inner: crate::conclusion::ConclusionRepresentationBuilder,
}

impl BlockingConclusionRepresentationBuilder {
    /// Semantic search query.
    #[must_use]
    pub fn search_query(self, val: impl Into<String>) -> Self {
        Self {
            inner: self.inner.search_query(val),
        }
    }

    /// Top-K (1–100).
    #[must_use]
    pub fn search_top_k(self, val: u32) -> Self {
        Self {
            inner: self.inner.search_top_k(val),
        }
    }

    /// Max cosine distance (0.0–1.0).
    #[must_use]
    pub fn search_max_distance(self, val: f64) -> Self {
        Self {
            inner: self.inner.search_max_distance(val),
        }
    }

    /// Include most frequent conclusions.
    #[must_use]
    pub fn include_most_frequent(self, val: bool) -> Self {
        Self {
            inner: self.inner.include_most_frequent(val),
        }
    }

    /// Max conclusions (1–100).
    #[must_use]
    pub fn max_conclusions(self, val: u32) -> Self {
        Self {
            inner: self.inner.max_conclusions(val),
        }
    }

    /// Send the request.
    ///
    /// # Errors
    ///
    /// Returns an [`HonchoError`](crate::error::HonchoError) variant if the
    /// underlying HTTP request fails (network, authentication, validation, or
    /// server-side errors).
    ///
    /// Also returns
    /// [`HonchoError::Configuration`](crate::error::HonchoError::Configuration)
    /// when invoked from inside a tokio/async runtime; in that case use the
    /// async [`crate::conclusion::ConclusionRepresentationBuilder`] instead.
    pub fn send(self) -> Result<String> {
        block_on(self.inner.send())?
    }
}

impl std::fmt::Debug for BlockingConclusionRepresentationBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlockingConclusionRepresentationBuilder")
            .finish_non_exhaustive()
    }
}

/// Blocking builder for listing conclusions.
///
/// Not `Clone`: the wrapped async
/// [`crate::conclusion::ListConclusionsBuilder`] is itself not `Clone`.
/// Rebuild from [`ConclusionScope::list`] for retries.
pub struct BlockingListConclusionsBuilder {
    inner: crate::conclusion::ListConclusionsBuilder,
}

impl BlockingListConclusionsBuilder {
    /// Page number (1-based).
    #[must_use]
    pub fn page(self, page: u32) -> Self {
        Self {
            inner: self.inner.page(page),
        }
    }

    /// Page size. Must be in `1..=100`.
    #[must_use]
    pub fn size(self, size: u32) -> Self {
        Self {
            inner: self.inner.size(size),
        }
    }

    /// Filter to a session.
    #[must_use]
    pub fn session(self, session_id: impl Into<String>) -> Self {
        Self {
            inner: self.inner.session(session_id),
        }
    }

    /// Reverse ordering.
    #[must_use]
    pub fn reverse(self, reverse: bool) -> Self {
        Self {
            inner: self.inner.reverse(reverse),
        }
    }

    /// Additional free-form filter criteria. See
    /// [`ListConclusionsBuilder::filters`](crate::conclusion::ListConclusionsBuilder::filters).
    #[must_use]
    pub fn filters(self, filters: HashMap<String, serde_json::Value>) -> Self {
        Self {
            inner: self.inner.filters(filters),
        }
    }

    /// Send and return paginated result.
    ///
    /// # Errors
    ///
    /// Returns an [`HonchoError`](crate::error::HonchoError) variant if the
    /// underlying HTTP request fails (network, authentication, validation, or
    /// server-side errors).
    ///
    /// Also returns
    /// [`HonchoError::Configuration`](crate::error::HonchoError::Configuration)
    /// when invoked from inside a tokio/async runtime; in that case use the
    /// async [`crate::conclusion::ListConclusionsBuilder`] instead.
    pub fn send(self) -> Result<ConclusionPage> {
        block_on(self.inner.send())?
    }
}

impl std::fmt::Debug for BlockingListConclusionsBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlockingListConclusionsBuilder")
            .finish_non_exhaustive()
    }
}

/// Blocking builder for querying conclusions.
///
/// Not `Clone`: the wrapped async
/// [`crate::conclusion::QueryConclusionsBuilder`] is itself not `Clone`.
/// Rebuild from [`ConclusionScope::query`] for retries.
pub struct BlockingQueryConclusionsBuilder {
    inner: crate::conclusion::QueryConclusionsBuilder,
}

impl BlockingQueryConclusionsBuilder {
    /// Number of results (1–100).
    #[must_use]
    pub fn top_k(self, top_k: u32) -> Self {
        Self {
            inner: self.inner.top_k(top_k),
        }
    }

    /// Max cosine distance threshold.
    #[must_use]
    pub fn distance(self, distance: f64) -> Self {
        Self {
            inner: self.inner.distance(distance),
        }
    }

    /// Additional free-form filter criteria. See
    /// [`QueryConclusionsBuilder::filters`](crate::conclusion::QueryConclusionsBuilder::filters).
    #[must_use]
    pub fn filters(self, filters: HashMap<String, serde_json::Value>) -> Self {
        Self {
            inner: self.inner.filters(filters),
        }
    }

    /// Send the query.
    ///
    /// # Errors
    ///
    /// Returns an [`HonchoError`](crate::error::HonchoError) variant if the
    /// underlying HTTP request fails (network, authentication, validation, or
    /// server-side errors).
    ///
    /// Also returns
    /// [`HonchoError::Configuration`](crate::error::HonchoError::Configuration)
    /// when invoked from inside a tokio/async runtime; in that case use the
    /// async [`crate::conclusion::QueryConclusionsBuilder`] instead.
    pub fn send(self) -> Result<Vec<Conclusion>> {
        block_on(self.inner.send())?.map(|v| v.into_iter().map(Conclusion::new).collect())
    }
}

impl std::fmt::Debug for BlockingQueryConclusionsBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlockingQueryConclusionsBuilder")
            .finish_non_exhaustive()
    }
}
