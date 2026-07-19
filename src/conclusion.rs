//! Conclusion wrapper and scoped access.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::error::{HonchoError, Result};
use crate::http::client::HttpClient;
use crate::http::routes;
use crate::types::conclusion::ConclusionLevel;
use crate::types::conclusion::ConclusionPage;
use crate::types::conclusion::ConclusionResponse as ConclusionData;
use crate::types::conclusion::{ConclusionGet, ConclusionQuery};
use crate::types::dialectic::RepresentationResponse;
use crate::types::pagination::paginate_post;
use crate::types::session::validate_search_params;

pub(crate) struct ConclusionInner {
    workspace_id: Arc<str>,
    id: String,
    content: String,
    observer_id: String,
    observed_id: String,
    session_id: Option<String>,
    created_at: DateTime<Utc>,
    level: ConclusionLevel,
}

/// A conclusion about a peer, produced by observation.
///
/// Wraps the API response and provides field accessors.
#[derive(Clone)]
pub struct Conclusion {
    inner: Arc<ConclusionInner>,
}

impl Conclusion {
    #[allow(clippy::needless_pass_by_value)]
    pub(crate) fn from_parts(workspace_id: impl Into<Arc<str>>, resp: ConclusionData) -> Self {
        Self {
            inner: Arc::new(ConclusionInner {
                workspace_id: workspace_id.into(),
                id: resp.id,
                content: resp.content,
                observer_id: resp.observer_id,
                observed_id: resp.observed_id,
                session_id: resp.session_id,
                created_at: resp.created_at,
                level: resp.level,
            }),
        }
    }

    /// The conclusion's unique identifier.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(c: &honcho_ai::Conclusion) {
    /// assert!(!c.id().is_empty());
    /// # }
    /// ```
    #[must_use]
    pub fn id(&self) -> &str {
        &self.inner.id
    }

    /// The conclusion content text.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(c: &honcho_ai::Conclusion) {
    /// println!("{}", c.content());
    /// # }
    /// ```
    #[must_use]
    pub fn content(&self) -> &str {
        &self.inner.content
    }

    /// ID of the peer that made this observation.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(c: &honcho_ai::Conclusion) {
    /// let observer = c.observer_id();
    /// # }
    /// ```
    #[must_use]
    pub fn observer_id(&self) -> &str {
        &self.inner.observer_id
    }

    /// ID of the peer being observed.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(c: &honcho_ai::Conclusion) {
    /// let observed = c.observed_id();
    /// # }
    /// ```
    #[must_use]
    pub fn observed_id(&self) -> &str {
        &self.inner.observed_id
    }

    /// Optional session this conclusion is scoped to.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(c: &honcho_ai::Conclusion) {
    /// if let Some(sid) = c.session_id() {
    ///     println!("scoped to session {sid}");
    /// }
    /// # }
    /// ```
    #[must_use]
    pub fn session_id(&self) -> Option<&str> {
        self.inner.session_id.as_deref()
    }

    /// When this conclusion was created.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(c: &honcho_ai::Conclusion) {
    /// let ts = c.created_at();
    /// # }
    /// ```
    #[must_use]
    pub fn created_at(&self) -> DateTime<Utc> {
        // `DateTime<Utc>` is `Copy`, so return by value rather than by reference.
        self.inner.created_at
    }

    /// Reasoning level of this conclusion (`explicit`, `deductive`,
    /// `inductive`, or `contradiction`). `explicit` = extracted from
    /// messages; the rest are derived during dreaming.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use honcho_ai::ConclusionLevel;
    /// # fn example(c: &honcho_ai::Conclusion) {
    /// if c.level() == ConclusionLevel::Contradiction {
    ///     println!("contradiction recorded");
    /// }
    /// # }
    /// ```
    #[must_use]
    pub fn level(&self) -> ConclusionLevel {
        // `ConclusionLevel` is `Copy`, so return by value rather than by reference.
        self.inner.level
    }

    /// The workspace this conclusion belongs to.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(c: &honcho_ai::Conclusion) {
    /// assert!(!c.workspace_id().is_empty());
    /// # }
    /// ```
    #[must_use]
    pub fn workspace_id(&self) -> &str {
        &self.inner.workspace_id
    }
}

impl fmt::Debug for Conclusion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        /// Maximum number of characters of `content` shown in debug output.
        const MAX_CHARS: usize = 50;
        let content = self.inner.content.as_str();
        // Char-based truncation (not byte-based) so multibyte UTF-8 is never
        // split; append an ellipsis marker when content is elided.
        let truncated: std::borrow::Cow<'_, str> = match content.char_indices().nth(MAX_CHARS) {
            Some((byte_idx, _)) => std::borrow::Cow::Owned(format!("{}…", &content[..byte_idx])),
            None => std::borrow::Cow::Borrowed(content),
        };
        f.debug_struct("Conclusion")
            .field("id", &self.inner.id)
            .field("content", &truncated)
            .finish_non_exhaustive()
    }
}

impl fmt::Display for Conclusion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.inner.content)
    }
}

/// Parameters for creating a single conclusion.
///
/// Use [`ConclusionCreateParams::new()`] for the common case, or the
/// [`bon::Builder`]–generated builder for optional fields.
#[derive(Debug, Clone, Serialize, bon::Builder)]
#[builder(on(String, into))]
#[builder(finish_fn = build)]
pub struct ConclusionCreateParams {
    /// The conclusion content text.
    pub(crate) content: String,
    /// Optional session ID to associate the conclusion with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) session_id: Option<String>,
}

impl ConclusionCreateParams {
    /// Shortcut: create params with content only (no session).
    ///
    /// # Examples
    ///
    /// ```
    /// use honcho_ai::ConclusionCreateParams;
    ///
    /// let params = ConclusionCreateParams::new("enjoys hiking");
    /// ```
    #[must_use]
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            session_id: None,
        }
    }
}

/// Owned per-item create payload.
///
/// Holds only the fields that vary per item; `observer_id` / `observed_id`
/// come from the scope and are injected by reference at serialization time
/// (see [`CreateRef`]), so the scope IDs are never cloned per item.
struct ConclusionItem {
    content: String,
    session_id: Option<String>,
}

/// Borrowing view of a single conclusion create body.
///
/// Serializes identically to `ConclusionCreate` but borrows every field, so
/// no per-item `String` is cloned.
#[derive(Serialize)]
struct CreateRef<'a> {
    content: &'a str,
    observer_id: &'a str,
    observed_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<&'a str>,
}

/// Borrowing view of a batch create body.
///
/// Serializes identically to `ConclusionBatchCreate`; built per chunk by
/// slicing the owned items rather than copying them (no `Vec::to_vec`).
#[derive(Serialize)]
struct BatchRef<'a> {
    conclusions: Vec<CreateRef<'a>>,
}

/// Build a borrowing batch body for one chunk of items.
// `observer`/`observed` are the domain field names; the similarity is inherent.
#[allow(clippy::similar_names)]
fn batch_ref<'a>(
    chunk: &'a [ConclusionItem],
    observer_id: &'a str,
    observed_id: &'a str,
) -> BatchRef<'a> {
    BatchRef {
        conclusions: chunk
            .iter()
            .map(|it| CreateRef {
                content: &it.content,
                observer_id,
                observed_id,
                session_id: it.session_id.as_deref(),
            })
            .collect(),
    }
}

pub(crate) struct ConclusionScopeInner {
    http: HttpClient,
    workspace_id: Arc<str>,
    observer: String,
    observed: String,
}

/// Scoped access to conclusions for a specific observer/observed relationship.
///
/// Typically obtained via `peer.conclusions()` (self-scoped) or
/// `peer.conclusions_of(target)` (cross-peer). Clone is cheap (Arc-backed).
#[derive(Clone)]
pub struct ConclusionScope {
    inner: Arc<ConclusionScopeInner>,
}

impl ConclusionScope {
    #[allow(clippy::similar_names)]
    pub(crate) fn new(
        http: HttpClient,
        workspace_id: String,
        observer_id: String,
        observed_id: String,
    ) -> Self {
        Self {
            inner: Arc::new(ConclusionScopeInner {
                http,
                workspace_id: workspace_id.into(),
                observer: observer_id,
                observed: observed_id,
            }),
        }
    }

    /// The observer peer ID for this scope.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(scope: &honcho_ai::ConclusionScope) {
    /// println!("observer: {}", scope.observer_id());
    /// # }
    /// ```
    #[must_use]
    pub fn observer_id(&self) -> &str {
        &self.inner.observer
    }

    /// The observed peer ID for this scope.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(scope: &honcho_ai::ConclusionScope) {
    /// println!("observed: {}", scope.observed_id());
    /// # }
    /// ```
    #[must_use]
    pub fn observed_id(&self) -> &str {
        &self.inner.observed
    }

    /// Create one or more conclusions in this scope.
    ///
    /// Auto-injects `observer_id` and `observed_id` from the scope. If more
    /// than 100 conclusions are provided they are sent in batches of 100.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(scope: &honcho_ai::ConclusionScope) -> honcho_ai::error::Result<()> {
    /// use honcho_ai::ConclusionCreateParams;
    /// let created = scope.create([
    ///     ConclusionCreateParams::new("likes coffee"),
    ///     ConclusionCreateParams::new("early riser"),
    /// ]).await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError::Server`] if the server rejects any batch.
    ///
    /// # Partial failure (inputs > 100)
    ///
    /// When more than 100 conclusions are supplied they are split into batches
    /// of 100 and sent **sequentially and non-atomically**. If a later batch
    /// fails, the conclusions from earlier batches have *already been created
    /// server-side* and are **not** rolled back — but their returned IDs are
    /// dropped and this method returns only the error, so the partial success
    /// is not reported to the caller. Callers needing all-or-nothing or
    /// exactly-once semantics for large inputs should chunk the input
    /// themselves (≤ 100 per call) and track which calls succeeded.
    ///
    /// A typed partial-result that carries the already-created conclusions
    /// alongside the error is planned for a future release.
    #[allow(clippy::similar_names)]
    pub async fn create(
        &self,
        conclusions: impl IntoIterator<Item = impl Into<ConclusionCreateParams>>,
    ) -> Result<Vec<Conclusion>> {
        let items: Vec<ConclusionItem> = conclusions
            .into_iter()
            .map(|c| {
                let p: ConclusionCreateParams = c.into();
                ConclusionItem {
                    content: p.content,
                    session_id: p.session_id,
                }
            })
            .collect();

        if items.is_empty() {
            return Ok(Vec::new());
        }

        let route = routes::conclusions(&self.inner.workspace_id)?;
        let observer = self.inner.observer.as_str();
        let observed = self.inner.observed.as_str();

        let all_data: Vec<ConclusionData> = if items.len() <= 100 {
            let body = batch_ref(&items, observer, observed);
            self.inner.http.post(&route, Some(&body), &[]).await?
        } else {
            // NOTE: batches are sent sequentially and are NOT atomic — see the
            // partial-failure hazard documented on this method. On `?` here the
            // already-created earlier chunks are accumulated in `all` but cannot
            // be surfaced through the current return type, so they are dropped.
            let mut all = Vec::with_capacity(items.len());
            for chunk in items.chunks(100) {
                let body = batch_ref(chunk, observer, observed);
                let batch: Vec<ConclusionData> =
                    self.inner.http.post(&route, Some(&body), &[]).await?;
                all.extend(batch);
            }
            all
        };

        Ok(all_data
            .into_iter()
            .map(|d| Conclusion::from_parts(self.inner.workspace_id.clone(), d))
            .collect())
    }

    /// Return a builder for fetching the scoped representation.
    ///
    /// **GOTCHA (C41):** This hits the *peer* representation endpoint, not the
    /// conclusion endpoint — `POST /v3/workspaces/{ws}/peers/{observer}/representation`
    /// with `target: observed_id`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(scope: &honcho_ai::ConclusionScope) -> honcho_ai::error::Result<()> {
    /// let rep = scope.representation().send().await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// The builder's `.send()` returns [`HonchoError::Validation`] if
    /// `search_top_k` ∉ [1, 100], `search_max_distance` ∉ [0.0, 1.0],
    /// or `max_conclusions` ∉ [1, 100]. Returns [`HonchoError::Server`] on
    /// transport or API errors.
    #[must_use]
    pub fn representation(&self) -> ConclusionRepresentationBuilder {
        ConclusionRepresentationBuilder {
            http: self.inner.http.clone(),
            workspace_id: self.inner.workspace_id.clone(),
            observer_id: self.inner.observer.clone(),
            observed_id: self.inner.observed.clone(),
            search_query: None,
            search_top_k: None,
            search_max_distance: None,
            include_most_frequent: None,
            max_conclusions: None,
        }
    }

    /// Return a builder for listing conclusions in this scope (paginated).
    ///
    /// Defaults: page 1, size 50, ascending order, no session filter.
    /// Chain `.session()`, `.page()`, `.size()`, `.reverse()` to customise,
    /// then call `.send()` to execute.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(scope: &honcho_ai::ConclusionScope) -> honcho_ai::error::Result<()> {
    /// let page = scope.list().page(1).size(20).send().await?;
    /// for c in page.items() {
    ///     println!("{}", c.content);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError::Server`] if the server rejects the request.
    pub fn list(&self) -> ListConclusionsBuilder {
        ListConclusionsBuilder {
            scope: self.clone(),
            page: 1,
            size: 50,
            session_id: None,
            reverse: false,
            filters: None,
        }
    }

    /// Return a builder for semantically querying conclusions in this scope.
    ///
    /// Defaults: `top_k` = 10, no distance threshold.
    /// Chain `.top_k()` and `.distance()` to customise, then call `.send()`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(scope: &honcho_ai::ConclusionScope) -> honcho_ai::error::Result<()> {
    /// let results = scope.query("hobbies").top_k(5).send().await?;
    /// for c in &results {
    ///     println!("{}", c.content());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError::Validation`] if `query` is empty, `top_k` ∉ [1, 100],
    /// or `distance` ∉ [0.0, 1.0]. Returns [`HonchoError::Server`] on
    /// transport or API errors.
    pub fn query(&self, query: impl Into<String>) -> QueryConclusionsBuilder {
        QueryConclusionsBuilder {
            scope: self.clone(),
            query: query.into(),
            top_k: 10,
            distance: None,
            filters: None,
        }
    }

    /// Delete a conclusion by ID.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(scope: &honcho_ai::ConclusionScope) -> honcho_ai::error::Result<()> {
    /// scope.delete("conc-42").await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError::Server`] if the conclusion does not exist or
    /// the server rejects the request.
    pub async fn delete(&self, conclusion_id: impl Into<String>) -> Result<()> {
        let route = routes::conclusion_delete(&self.inner.workspace_id, &conclusion_id.into())?;
        self.inner.http.delete(&route, &[]).await
    }
}

/// Builder for scoped representation requests on a [`ConclusionScope`].
///
/// Obtained via [`ConclusionScope::representation()`].
pub struct ConclusionRepresentationBuilder {
    http: HttpClient,
    workspace_id: Arc<str>,
    observer_id: String,
    observed_id: String,
    search_query: Option<String>,
    search_top_k: Option<u32>,
    search_max_distance: Option<f64>,
    include_most_frequent: Option<bool>,
    max_conclusions: Option<u32>,
}

impl ConclusionRepresentationBuilder {
    /// Semantic search query to curate the representation.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(scope: &honcho_ai::ConclusionScope) {
    /// let _builder = scope.representation().search_query("preferences");
    /// # }
    /// ```
    #[must_use]
    pub fn search_query(mut self, val: impl Into<String>) -> Self {
        self.search_query = Some(val.into());
        self
    }

    /// Number of semantic-search-retrieved conclusions (1–100).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(scope: &honcho_ai::ConclusionScope) {
    /// let _builder = scope.representation().search_top_k(20);
    /// # }
    /// ```
    #[must_use]
    pub fn search_top_k(mut self, val: u32) -> Self {
        self.search_top_k = Some(val);
        self
    }

    /// Maximum cosine distance for semantically relevant conclusions (0.0–1.0).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(scope: &honcho_ai::ConclusionScope) {
    /// let _builder = scope.representation().search_max_distance(0.5);
    /// # }
    /// ```
    #[must_use]
    pub fn search_max_distance(mut self, val: f64) -> Self {
        self.search_max_distance = Some(val);
        self
    }

    /// Whether to include the most frequent conclusions.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(scope: &honcho_ai::ConclusionScope) {
    /// let _builder = scope.representation().include_most_frequent(true);
    /// # }
    /// ```
    #[must_use]
    pub fn include_most_frequent(mut self, val: bool) -> Self {
        self.include_most_frequent = Some(val);
        self
    }

    /// Maximum number of conclusions to include (1–100).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(scope: &honcho_ai::ConclusionScope) {
    /// let _builder = scope.representation().max_conclusions(25);
    /// # }
    /// ```
    #[must_use]
    pub fn max_conclusions(mut self, val: u32) -> Self {
        self.max_conclusions = Some(val);
        self
    }

    /// Send the representation request.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(scope: &honcho_ai::ConclusionScope) -> honcho_ai::error::Result<()> {
    /// let rep = scope.representation()
    ///     .search_query("hobbies")
    ///     .search_top_k(10)
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError::Validation`]
    /// if `search_top_k`, `search_max_distance`, or `max_conclusions` are out of range.
    pub async fn send(self) -> Result<String> {
        validate_search_params(
            self.search_top_k,
            self.search_max_distance,
            self.max_conclusions,
        )?;

        let params = crate::types::peer::PeerRepresentationGet {
            session_id: None,
            target: Some(self.observed_id),
            search_query: self.search_query,
            search_top_k: self.search_top_k,
            search_max_distance: self.search_max_distance,
            include_most_frequent: self.include_most_frequent,
            max_conclusions: self.max_conclusions,
        };

        let route = routes::peer_representation(&self.workspace_id, &self.observer_id)?;
        let resp: RepresentationResponse = self.http.post(&route, Some(&params), &[]).await?;
        Ok(resp.representation)
    }
}

/// Builder for paginated conclusion listing, obtained via [`ConclusionScope::list()`].
#[must_use]
pub struct ListConclusionsBuilder {
    scope: ConclusionScope,
    page: u64,
    size: u64,
    session_id: Option<String>,
    reverse: bool,
    filters: Option<HashMap<String, serde_json::Value>>,
}

/// Keys managed by [`ConclusionScope::list`] / [`ConclusionScope::query`]: they
/// derive from the scope's observer/observed pair (and `.session()` on `list`),
/// so a caller passing them in `filters` would silently override the scope.
/// `query` does **not** reject `session` / `session_id` because it has no
/// `.session()` method — `session_id` is a legitimate caller filter there.
const LIST_RESERVED_FILTERS: [&str; 6] = [
    "observer",
    "observed",
    "observer_id",
    "observed_id",
    "session",
    "session_id",
];
const QUERY_RESERVED_FILTERS: [&str; 4] = ["observer", "observed", "observer_id", "observed_id"];

/// Reject caller-supplied filter keys that are managed by the scope.
///
/// Returns `Ok(())` if `filters` is `None` or none of its keys clash with
/// `reserved`; otherwise returns [`HonchoError::Validation`] listing the
/// offending keys (sorted, machine-readable) with API-surface guidance.
fn reject_reserved_filters(
    filters: Option<&HashMap<String, serde_json::Value>>,
    reserved: &[&str],
    op: &str,
) -> Result<()> {
    let Some(f) = filters else {
        return Ok(());
    };
    let mut clash: Vec<&str> = reserved
        .iter()
        .copied()
        .filter(|k| f.contains_key(*k))
        .collect();
    clash.sort_unstable();
    if clash.is_empty() {
        return Ok(());
    }
    let guidance = if op == "list" {
        " Use .session() to filter by session."
    } else {
        ""
    };
    Err(HonchoError::Validation(format!(
        "Filter key(s) {clash:?} are managed by this conclusion scope and cannot be passed in \
         filters. Choose the peer pair via peer.conclusions / peer.conclusions_of(target).{guidance}"
    )))
}

impl ListConclusionsBuilder {
    /// Set the page number (1-indexed, default 1).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(scope: &honcho_ai::ConclusionScope) {
    /// let _builder = scope.list().page(2);
    /// # }
    /// ```
    pub fn page(mut self, page: u32) -> Self {
        self.page = u64::from(page);
        self
    }

    /// Set the page size (default 50, must be in `1..=100`).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(scope: &honcho_ai::ConclusionScope) {
    /// let _builder = scope.list().size(25);
    /// # }
    /// ```
    pub fn size(mut self, size: u32) -> Self {
        self.size = u64::from(size);
        self
    }

    /// Filter conclusions to a specific session.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(scope: &honcho_ai::ConclusionScope) {
    /// let _builder = scope.list().session("sess-42");
    /// # }
    /// ```
    pub fn session(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    /// Additional free-form filter criteria (e.g. `{"level": "explicit"}`).
    ///
    /// Merged on top of the scope's `observer_id` / `observed_id` (and the
    /// `session_id` injected by [`.session()`](Self::session)). The reserved
    /// scope-managed keys (`observer`, `observed`, `observer_id`, `observed_id`,
    /// `session`, `session_id`) are rejected on [`.send()`](Self::send) — use
    /// `.session()` for session filtering, and pick the peer pair via
    /// `peer.conclusions()` / `peer.conclusions_of(target)`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(scope: &honcho_ai::ConclusionScope) {
    /// use std::collections::HashMap;
    /// let mut filters = HashMap::new();
    /// filters.insert("level".to_owned(), serde_json::json!("explicit"));
    /// let _builder = scope.list().filters(filters);
    /// # }
    /// ```
    pub fn filters(mut self, filters: HashMap<String, serde_json::Value>) -> Self {
        self.filters = Some(filters);
        self
    }

    /// Reverse the default ordering.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(scope: &honcho_ai::ConclusionScope) {
    /// let _builder = scope.list().reverse(true);
    /// # }
    /// ```
    pub fn reverse(mut self, reverse: bool) -> Self {
        self.reverse = reverse;
        self
    }

    /// Send the list request and return a paginated result.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(scope: &honcho_ai::ConclusionScope) -> honcho_ai::error::Result<()> {
    /// let page = scope.list().page(1).size(20).send().await?;
    /// println!("total: {}", page.total());
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError::Validation`] if `filters` contains any
    /// scope-managed key (`observer`, `observed`, `observer_id`, `observed_id`,
    /// `session`, `session_id`). Returns [`HonchoError::Server`] if the server
    /// rejects the request.
    pub async fn send(self) -> Result<ConclusionPage> {
        reject_reserved_filters(self.filters.as_ref(), &LIST_RESERVED_FILTERS, "list")?;
        let mut filters = self.filters.unwrap_or_default();
        filters.insert(
            "observer_id".to_owned(),
            serde_json::json!(self.scope.inner.observer.as_str()),
        );
        filters.insert(
            "observed_id".to_owned(),
            serde_json::json!(self.scope.inner.observed.as_str()),
        );
        if let Some(sid) = self.session_id {
            filters.insert("session_id".to_owned(), serde_json::json!(sid));
        }
        let body = ConclusionGet::builder().filters(filters).build();
        let body = serde_json::to_value(&body).map_err(|e| HonchoError::Serialization {
            path: "ConclusionGet".to_owned(),
            source: e,
        })?;
        let route = routes::conclusions_list(&self.scope.inner.workspace_id)?;
        paginate_post(
            &self.scope.inner.http,
            &route,
            Some(&body),
            self.page,
            self.size,
            self.reverse,
        )
        .await
    }
}

/// Builder for semantic conclusion queries, obtained via [`ConclusionScope::query()`].
#[must_use]
pub struct QueryConclusionsBuilder {
    scope: ConclusionScope,
    query: String,
    top_k: u32,
    distance: Option<f64>,
    filters: Option<HashMap<String, serde_json::Value>>,
}

impl QueryConclusionsBuilder {
    /// Set the number of results (1–100, default 10).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(scope: &honcho_ai::ConclusionScope) {
    /// let _builder = scope.query("interests").top_k(5);
    /// # }
    /// ```
    pub fn top_k(mut self, top_k: u32) -> Self {
        self.top_k = top_k;
        self
    }

    /// Set the maximum cosine distance threshold (0.0–1.0).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(scope: &honcho_ai::ConclusionScope) {
    /// let _builder = scope.query("interests").distance(0.7);
    /// # }
    /// ```
    pub fn distance(mut self, distance: f64) -> Self {
        self.distance = Some(distance);
        self
    }

    /// Additional free-form filter criteria (e.g. `{"level": "deductive"}`).
    ///
    /// Merged on top of the scope's `observer_id` / `observed_id`. Because this
    /// builder has no [`.session()`](ConclusionScope::list) method, `session_id`
    /// is a legitimate caller-supplied filter here. The reserved peer-pair keys
    /// (`observer`, `observed`, `observer_id`, `observed_id`) are rejected on
    /// [`.send()`](Self::send) — pick the pair via `peer.conclusions()` /
    /// `peer.conclusions_of(target)`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(scope: &honcho_ai::ConclusionScope) {
    /// use std::collections::HashMap;
    /// let mut filters = HashMap::new();
    /// filters.insert("level".to_owned(), serde_json::json!("deductive"));
    /// let _builder = scope.query("hobbies").filters(filters);
    /// # }
    /// ```
    pub fn filters(mut self, filters: HashMap<String, serde_json::Value>) -> Self {
        self.filters = Some(filters);
        self
    }

    /// Send the query request.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(scope: &honcho_ai::ConclusionScope) -> honcho_ai::error::Result<()> {
    /// let results = scope.query("preferences").top_k(5).distance(0.8).send().await?;
    /// for c in &results {
    ///     println!("{}", c.content());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError::Validation`] if `query` is empty, `top_k` ∉ [1, 100],
    /// `distance` ∉ [0.0, 1.0], or `filters` contains any of the scope-managed
    /// keys (`observer`, `observed`, `observer_id`, `observed_id`).
    pub async fn send(self) -> Result<Vec<Conclusion>> {
        if self.query.trim().is_empty() {
            return Err(HonchoError::Validation(
                "query must not be empty".to_string(),
            ));
        }
        validate_search_params(Some(self.top_k), self.distance, None).map_err(|e| {
            if let HonchoError::Validation(msg) = &e {
                // Remap the internal representation-param names to this builder's
                // public method names so error messages reference `top_k` /
                // `distance`, not `search_top_k` / `search_max_distance`.
                HonchoError::Validation(
                    msg.replace("search_max_distance", "distance")
                        .replace("search_top_k", "top_k"),
                )
            } else {
                e
            }
        })?;
        reject_reserved_filters(self.filters.as_ref(), &QUERY_RESERVED_FILTERS, "query")?;
        let mut filters = self.filters.unwrap_or_default();
        filters.insert(
            "observer_id".to_owned(),
            serde_json::json!(self.scope.inner.observer.as_str()),
        );
        filters.insert(
            "observed_id".to_owned(),
            serde_json::json!(self.scope.inner.observed.as_str()),
        );
        let body = ConclusionQuery::builder()
            .query(self.query)
            .top_k(self.top_k)
            .maybe_distance(self.distance)
            .filters(filters)
            .build();
        let body = serde_json::to_value(&body).map_err(|e| HonchoError::Serialization {
            path: "ConclusionQuery".to_owned(),
            source: e,
        })?;
        let route = routes::conclusions_query(&self.scope.inner.workspace_id)?;
        let data: Vec<ConclusionData> =
            self.scope.inner.http.post(&route, Some(&body), &[]).await?;
        let ws = self.scope.inner.workspace_id.clone();
        Ok(data
            .into_iter()
            .map(|d| Conclusion::from_parts(ws.clone(), d))
            .collect())
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unnecessary_wraps,
    clippy::needless_pass_by_value,
    clippy::unused_async
)]
mod tests {
    use super::*;
    use crate::http::client::HttpClient;
    use wiremock::matchers::{body_json, body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn make_scope(server: &MockServer) -> ConclusionScope {
        let http =
            HttpClient::from_params(HttpClient::builder().base_url(server.uri()).build()).unwrap();
        ConclusionScope::new(http, "ws1".to_owned(), "alice".to_owned(), "bob".to_owned())
    }

    fn conclusion_json(content: &str, id: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "content": content,
            "observer_id": "alice",
            "observed_id": "bob",
            "session_id": null,
            "created_at": "2025-01-15T10:30:00Z",
            "level": "explicit",
        })
    }

    fn conclusion_json_with_session(
        content: &str,
        id: &str,
        session_id: &str,
    ) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "content": content,
            "observer_id": "alice",
            "observed_id": "bob",
            "session_id": session_id,
            "created_at": "2025-01-15T10:30:00Z",
            "level": "explicit",
        })
    }

    #[test]
    fn create_params_minimal_serializes_content_only() {
        let params = ConclusionCreateParams::new("hello");
        let json = serde_json::to_value(params).unwrap();
        assert_eq!(json["content"], "hello");
        assert!(json.get("session_id").is_none());
    }

    #[test]
    fn create_params_with_session_id_serializes_both() {
        let params = ConclusionCreateParams::builder()
            .content("world".to_owned())
            .session_id("s1".to_owned())
            .build();
        let json = serde_json::to_value(params).unwrap();
        assert_eq!(json["content"], "world");
        assert_eq!(json["session_id"], "s1");
    }

    #[test]
    fn debug_truncates_long_content() {
        let data = make_conclusion_data("a".repeat(80), None);
        let conc = Conclusion::from_parts("ws".to_owned(), data);
        let dbg = format!("{conc:?}");
        assert!(dbg.contains("Conclusion { id: \"c1\", content: \""));
        assert!(!dbg.contains(&"a".repeat(80)));
    }

    #[test]
    fn debug_truncation_multibyte_utf8() {
        let data = make_conclusion_data("\u{4e00}".repeat(60), None);
        let conc = Conclusion::from_parts("ws".to_owned(), data);
        let dbg = format!("{conc:?}");
        assert!(!dbg.contains(&"\u{4e00}".repeat(60)));
    }

    #[test]
    fn display_returns_full_content() {
        let long = "x".repeat(200);
        let data = make_conclusion_data(long.clone(), None);
        let conc = Conclusion::from_parts("ws".to_owned(), data);
        assert_eq!(format!("{conc}"), long);
    }

    #[test]
    fn getters_return_correct_values() {
        let data = make_conclusion_data("content here".to_owned(), Some("sess-1".to_owned()));
        let conc = Conclusion::from_parts("ws-1".to_owned(), data);
        assert_eq!(conc.id(), "c1");
        assert_eq!(conc.content(), "content here");
        assert_eq!(conc.observer_id(), "obs");
        assert_eq!(conc.observed_id(), "obd");
        assert_eq!(conc.session_id(), Some("sess-1"));
        assert_eq!(conc.workspace_id(), "ws-1");
    }

    fn make_conclusion_data(content: String, session_id: Option<String>) -> ConclusionData {
        ConclusionData {
            id: "c1".to_owned(),
            content,
            observer_id: "obs".to_owned(),
            observed_id: "obd".to_owned(),
            session_id,
            created_at: chrono::Utc::now(),
            level: ConclusionLevel::Explicit,
        }
    }

    fn test_http() -> HttpClient {
        HttpClient::from_params(
            HttpClient::builder()
                .base_url("http://localhost".to_owned())
                .build(),
        )
        .unwrap()
    }

    #[test]
    fn conclusion_scope_new_self_scoped() {
        let scope = ConclusionScope::new(
            test_http(),
            "ws".to_owned(),
            "p1".to_owned(),
            "p1".to_owned(),
        );
        assert_eq!(scope.observer_id(), "p1");
        assert_eq!(scope.observed_id(), "p1");
    }

    #[test]
    fn conclusion_scope_with_different_target() {
        let scope = ConclusionScope::new(
            test_http(),
            "ws".to_owned(),
            "alice".to_owned(),
            "bob".to_owned(),
        );
        assert_eq!(scope.observer_id(), "alice");
        assert_eq!(scope.observed_id(), "bob");
    }

    #[test]
    fn conclusion_scope_clone_is_cheap() {
        let scope =
            ConclusionScope::new(test_http(), "ws".to_owned(), "a".to_owned(), "b".to_owned());
        let clone = scope.clone();
        assert_eq!(Arc::strong_count(&scope.inner), 2);
        assert_eq!(clone.observer_id(), "a");
        assert_eq!(clone.observed_id(), "b");
        drop(clone);
        assert_eq!(Arc::strong_count(&scope.inner), 1);
    }

    #[test]
    fn conclusion_scope_construction_basic() {
        let scope = ConclusionScope::new(
            test_http(),
            "ws-99".to_owned(),
            "observer".to_owned(),
            "observed".to_owned(),
        );
        assert_eq!(scope.observer_id(), "observer");
        assert_eq!(scope.observed_id(), "observed");
    }

    // ── F9.6: ConclusionScope::create tests ──────────────────────────────

    #[tokio::test]
    async fn create_single_conclusion() {
        let server = MockServer::start().await;
        let scope = make_scope(&server);

        let expected_body = serde_json::json!({
            "conclusions": [{
                "content": "likes rust",
                "observer_id": "alice",
                "observed_id": "bob",
            }]
        });

        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/conclusions"))
            .and(body_json(&expected_body))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(vec![conclusion_json("likes rust", "c1")]),
            )
            .mount(&server)
            .await;

        let results = scope
            .create([ConclusionCreateParams::new("likes rust")])
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id(), "c1");
        assert_eq!(results[0].content(), "likes rust");
    }

    #[tokio::test]
    async fn create_injects_observer_and_observed() {
        let server = MockServer::start().await;
        let scope = make_scope(&server);

        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/conclusions"))
            .and(body_string_contains("\"observer_id\":\"alice\""))
            .and(body_string_contains("\"observed_id\":\"bob\""))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(vec![conclusion_json("test", "c1")]),
            )
            .mount(&server)
            .await;

        let results = scope
            .create([ConclusionCreateParams::new("test")])
            .await
            .unwrap();
        assert_eq!(results[0].observer_id(), "alice");
        assert_eq!(results[0].observed_id(), "bob");
    }

    #[tokio::test]
    async fn create_batch_under_100_one_request() {
        let server = MockServer::start().await;
        let scope = make_scope(&server);

        let responses: Vec<serde_json::Value> = (0..50)
            .map(|i| conclusion_json(&format!("conc-{i}"), &format!("id-{i}")))
            .collect();

        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/conclusions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&responses))
            .expect(1)
            .mount(&server)
            .await;

        let params: Vec<ConclusionCreateParams> = (0..50)
            .map(|i| ConclusionCreateParams::new(format!("conc-{i}")))
            .collect();
        let results = scope.create(params).await.unwrap();
        assert_eq!(results.len(), 50);
    }

    #[tokio::test]
    async fn create_batch_over_100_chunks_requests() {
        let server = MockServer::start().await;
        let scope = make_scope(&server);

        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/conclusions"))
            .respond_with(|req: &wiremock::Request| {
                let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
                let count = body["conclusions"].as_array().unwrap().len();
                let items: Vec<serde_json::Value> = (0..count)
                    .map(|i| conclusion_json(&format!("c-{i}"), &format!("id-{i}")))
                    .collect();
                ResponseTemplate::new(200).set_body_json(&items)
            })
            .expect(2)
            .mount(&server)
            .await;

        let params: Vec<ConclusionCreateParams> = (0..150)
            .map(|i| ConclusionCreateParams::new(format!("c-{i}")))
            .collect();
        let results = scope.create(params).await.unwrap();
        assert_eq!(results.len(), 150);
    }

    #[tokio::test]
    async fn create_batch_exactly_100_is_one_request() {
        let server = MockServer::start().await;
        let scope = make_scope(&server);

        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/conclusions"))
            .respond_with(|req: &wiremock::Request| {
                let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
                let count = body["conclusions"].as_array().unwrap().len();
                let items: Vec<serde_json::Value> = (0..count)
                    .map(|i| conclusion_json(&format!("c-{i}"), &format!("id-{i}")))
                    .collect();
                ResponseTemplate::new(200).set_body_json(&items)
            })
            .expect(1)
            .mount(&server)
            .await;

        let params: Vec<ConclusionCreateParams> = (0..100)
            .map(|i| ConclusionCreateParams::new(format!("c-{i}")))
            .collect();
        let results = scope.create(params).await.unwrap();
        assert_eq!(results.len(), 100);
    }

    // ── F9.7: ConclusionScope::representation tests ──────────────────────

    #[tokio::test]
    async fn representation_uses_peer_endpoint() {
        let server = MockServer::start().await;
        let scope = make_scope(&server);

        let expected_body = serde_json::json!({
            "target": "bob",
        });

        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/peers/alice/representation"))
            .and(body_json(&expected_body))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"representation": "Bob is friendly"})),
            )
            .mount(&server)
            .await;

        let rep = scope.representation().send().await.unwrap();
        assert_eq!(rep, "Bob is friendly");
    }

    #[tokio::test]
    async fn representation_with_search_options() {
        let server = MockServer::start().await;
        let scope = make_scope(&server);

        let expected_body = serde_json::json!({
            "target": "bob",
            "search_query": "preferences",
            "search_top_k": 5,
            "search_max_distance": 0.8,
            "include_most_frequent": true,
            "max_conclusions": 20,
        });

        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/peers/alice/representation"))
            .and(body_json(&expected_body))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"representation": "curated rep"})),
            )
            .mount(&server)
            .await;

        let rep = scope
            .representation()
            .search_query("preferences")
            .search_top_k(5)
            .search_max_distance(0.8)
            .include_most_frequent(true)
            .max_conclusions(20)
            .send()
            .await
            .unwrap();
        assert_eq!(rep, "curated rep");
    }

    // ── F9.3: ConclusionScope::list tests ────────────────────────────────

    fn page_json(items: Vec<serde_json::Value>) -> serde_json::Value {
        serde_json::json!({
            "items": items,
            "total": items.len(),
            "page": 1,
            "size": 50,
            "pages": 1,
        })
    }

    #[tokio::test]
    async fn list_sends_correct_filters() {
        let server = MockServer::start().await;
        let scope = make_scope(&server);

        let expected_body = serde_json::json!({
            "filters": {
                "observer_id": "alice",
                "observed_id": "bob",
            }
        });

        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/conclusions/list"))
            .and(body_json(&expected_body))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(page_json(vec![conclusion_json("likes tea", "c1")])),
            )
            .mount(&server)
            .await;

        let page = scope.list().send().await.unwrap();
        assert_eq!(page.total(), 1);
    }

    #[tokio::test]
    async fn list_with_session_filter() {
        let server = MockServer::start().await;
        let scope = make_scope(&server);

        let expected_body = serde_json::json!({
            "filters": {
                "observer_id": "alice",
                "observed_id": "bob",
                "session_id": "sess-42",
            }
        });

        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/conclusions/list"))
            .and(body_json(&expected_body))
            .respond_with(ResponseTemplate::new(200).set_body_json(page_json(vec![
                conclusion_json_with_session("scoped", "c2", "sess-42"),
            ])))
            .mount(&server)
            .await;

        let page = scope.list().session("sess-42").send().await.unwrap();
        assert_eq!(page.total(), 1);
    }

    #[tokio::test]
    async fn list_with_reverse() {
        let server = MockServer::start().await;
        let scope = make_scope(&server);

        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/conclusions/list"))
            .and(wiremock::matchers::query_param("reverse", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page_json(vec![])))
            .mount(&server)
            .await;

        let page = scope.list().reverse(true).send().await.unwrap();
        assert_eq!(page.total(), 0);
    }

    #[tokio::test]
    async fn list_default_page_size() {
        let server = MockServer::start().await;
        let scope = make_scope(&server);

        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/conclusions/list"))
            .and(wiremock::matchers::query_param("page", "1"))
            .and(wiremock::matchers::query_param("size", "50"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page_json(vec![])))
            .mount(&server)
            .await;

        let page = scope.list().send().await.unwrap();
        assert_eq!(page.page(), 1);
        assert_eq!(page.size(), 50);
    }

    // ── F9.4: ConclusionScope::query tests ───────────────────────────────

    #[tokio::test]
    async fn query_returns_conclusions() {
        let server = MockServer::start().await;
        let scope = make_scope(&server);

        let expected_body = serde_json::json!({
            "query": "preferences",
            "top_k": 10,
            "filters": {
                "observer_id": "alice",
                "observed_id": "bob",
            }
        });

        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/conclusions/query"))
            .and(body_json(&expected_body))
            .respond_with(ResponseTemplate::new(200).set_body_json(vec![
                conclusion_json("likes rust", "c1"),
                conclusion_json("prefers dark mode", "c2"),
            ]))
            .mount(&server)
            .await;

        let results = scope.query("preferences").send().await.unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id(), "c1");
        assert_eq!(results[1].id(), "c2");
    }

    #[tokio::test]
    async fn query_with_top_k_and_distance() {
        let server = MockServer::start().await;
        let scope = make_scope(&server);

        let expected_body = serde_json::json!({
            "query": "test",
            "top_k": 5,
            "distance": 0.7,
            "filters": {
                "observer_id": "alice",
                "observed_id": "bob",
            }
        });

        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/conclusions/query"))
            .and(body_json(&expected_body))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(vec![conclusion_json("result", "c1")]),
            )
            .mount(&server)
            .await;

        let results = scope
            .query("test")
            .top_k(5)
            .distance(0.7)
            .send()
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn query_validates_empty_query() {
        let scope =
            ConclusionScope::new(test_http(), "ws".to_owned(), "a".to_owned(), "b".to_owned());

        let err = scope.query("").send().await.unwrap_err();
        assert!(matches!(err, HonchoError::Validation(_)));
        assert_eq!(err.code(), "validation_error");
    }

    #[tokio::test]
    async fn query_validates_top_k_range() {
        let scope =
            ConclusionScope::new(test_http(), "ws".to_owned(), "a".to_owned(), "b".to_owned());

        let err = scope.query("test").top_k(0).send().await.unwrap_err();
        assert!(matches!(err, HonchoError::Validation(_)));
        assert_eq!(err.code(), "validation_error");

        let err = scope.query("test").top_k(101).send().await.unwrap_err();
        assert!(matches!(err, HonchoError::Validation(_)));
    }

    #[tokio::test]
    async fn query_validates_distance_range() {
        let scope =
            ConclusionScope::new(test_http(), "ws".to_owned(), "a".to_owned(), "b".to_owned());

        let err = scope.query("test").distance(-0.1).send().await.unwrap_err();
        assert!(matches!(err, HonchoError::Validation(_)));

        let err = scope.query("test").distance(1.1).send().await.unwrap_err();
        assert!(matches!(err, HonchoError::Validation(_)));
    }

    // ── filters(): reserved-key guard + merge behavior ──────────────────

    fn filters_pair(level: &str) -> HashMap<String, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("level".to_owned(), serde_json::json!(level));
        m
    }

    #[tokio::test]
    async fn list_with_filters_merges_level() {
        let server = MockServer::start().await;
        let scope = make_scope(&server);

        // Caller's `level` is layered on top of the scope's observer_id /
        // observed_id, all three must appear in the request body.
        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/conclusions/list"))
            .and(body_string_contains("\"observer_id\":\"alice\""))
            .and(body_string_contains("\"observed_id\":\"bob\""))
            .and(body_string_contains("\"level\":\"explicit\""))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(page_json(vec![conclusion_json("x", "c1")])),
            )
            .mount(&server)
            .await;

        let page = scope
            .list()
            .filters(filters_pair("explicit"))
            .send()
            .await
            .unwrap();
        assert_eq!(page.total(), 1);
    }

    #[tokio::test]
    async fn list_rejects_reserved_filter_observer_id() {
        let scope =
            ConclusionScope::new(test_http(), "ws".to_owned(), "a".to_owned(), "b".to_owned());
        let mut filters = HashMap::new();
        filters.insert("observer_id".to_owned(), serde_json::json!("x"));

        let err = scope.list().filters(filters).send().await.unwrap_err();
        assert!(matches!(err, HonchoError::Validation(_)));
        assert_eq!(err.code(), "validation_error");
    }

    #[tokio::test]
    async fn list_rejects_reserved_filter_observer() {
        let scope =
            ConclusionScope::new(test_http(), "ws".to_owned(), "a".to_owned(), "b".to_owned());
        let mut filters = HashMap::new();
        filters.insert("observer".to_owned(), serde_json::json!("x"));

        let err = scope.list().filters(filters).send().await.unwrap_err();
        assert!(matches!(err, HonchoError::Validation(_)));
        assert_eq!(err.code(), "validation_error");
    }

    #[tokio::test]
    async fn list_rejects_reserved_filter_observed() {
        let scope =
            ConclusionScope::new(test_http(), "ws".to_owned(), "a".to_owned(), "b".to_owned());
        let mut filters = HashMap::new();
        filters.insert("observed".to_owned(), serde_json::json!("x"));

        let err = scope.list().filters(filters).send().await.unwrap_err();
        assert!(matches!(err, HonchoError::Validation(_)));
        assert_eq!(err.code(), "validation_error");
    }

    #[tokio::test]
    async fn list_rejects_reserved_filter_session() {
        let scope =
            ConclusionScope::new(test_http(), "ws".to_owned(), "a".to_owned(), "b".to_owned());
        let mut filters = HashMap::new();
        filters.insert("session".to_owned(), serde_json::json!("x"));

        let err = scope.list().filters(filters).send().await.unwrap_err();
        let HonchoError::Validation(msg) = err else {
            panic!("expected validation error, got: {err:?}");
        };
        assert!(msg.contains("session"), "error should name the key: {msg}");
    }

    #[tokio::test]
    async fn list_rejects_reserved_filter_session_id() {
        let scope =
            ConclusionScope::new(test_http(), "ws".to_owned(), "a".to_owned(), "b".to_owned());
        let mut filters = HashMap::new();
        filters.insert("session_id".to_owned(), serde_json::json!("x"));

        let err = scope.list().filters(filters).send().await.unwrap_err();
        assert_eq!(err.code(), "validation_error");
        let HonchoError::Validation(msg) = &err else {
            panic!("expected validation error, got: {err:?}");
        };
        // `.session()` is the supported way to set session_id on `list`.
        assert!(
            msg.contains(".session()"),
            "error should point at .session(): {msg}"
        );
    }

    #[tokio::test]
    async fn query_with_filters_keeps_session_id() {
        let server = MockServer::start().await;
        let scope = make_scope(&server);

        // `query` has no `.session()`, so `session_id` is a legitimate caller
        // filter and must reach the wire alongside observer_id / observed_id /
        // the caller-supplied `level`.
        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/conclusions/query"))
            .and(body_string_contains("\"observer_id\":\"alice\""))
            .and(body_string_contains("\"observed_id\":\"bob\""))
            .and(body_string_contains("\"session_id\":\"s1\""))
            .and(body_string_contains("\"level\":\"deductive\""))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(vec![conclusion_json("x", "c1")]),
            )
            .mount(&server)
            .await;

        let mut filters = HashMap::new();
        filters.insert("session_id".to_owned(), serde_json::json!("s1"));
        filters.insert("level".to_owned(), serde_json::json!("deductive"));
        let results = scope
            .query("hobbies")
            .filters(filters)
            .send()
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn query_rejects_reserved_filter_observer_id() {
        let scope =
            ConclusionScope::new(test_http(), "ws".to_owned(), "a".to_owned(), "b".to_owned());
        let mut filters = HashMap::new();
        filters.insert("observer_id".to_owned(), serde_json::json!("x"));

        let err = scope.query("x").filters(filters).send().await.unwrap_err();
        assert!(matches!(err, HonchoError::Validation(_)));
        assert_eq!(err.code(), "validation_error");
    }

    #[tokio::test]
    async fn query_rejects_reserved_filter_observer() {
        let scope =
            ConclusionScope::new(test_http(), "ws".to_owned(), "a".to_owned(), "b".to_owned());
        let mut filters = HashMap::new();
        filters.insert("observer".to_owned(), serde_json::json!("x"));

        let err = scope.query("x").filters(filters).send().await.unwrap_err();
        assert!(matches!(err, HonchoError::Validation(_)));
        assert_eq!(err.code(), "validation_error");
    }

    #[tokio::test]
    async fn query_rejects_reserved_filter_observed() {
        let scope =
            ConclusionScope::new(test_http(), "ws".to_owned(), "a".to_owned(), "b".to_owned());
        let mut filters = HashMap::new();
        filters.insert("observed".to_owned(), serde_json::json!("x"));

        let err = scope.query("x").filters(filters).send().await.unwrap_err();
        assert!(matches!(err, HonchoError::Validation(_)));
        assert_eq!(err.code(), "validation_error");
    }

    #[tokio::test]
    async fn query_accepts_session_id_in_filters() {
        let server = MockServer::start().await;
        let scope = make_scope(&server);

        // Mirror of `query_with_filters_keeps_session_id` focused on the
        // accept-path: passing only `session_id` must not raise (the reserved
        // set for `query` excludes `session` / `session_id`).
        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/conclusions/query"))
            .and(body_string_contains("\"session_id\":\"sx\""))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&server)
            .await;

        let mut filters = HashMap::new();
        filters.insert("session_id".to_owned(), serde_json::json!("sx"));
        let results = scope.query("q").filters(filters).send().await.unwrap();
        assert!(results.is_empty());
    }

    // ── F9.5: ConclusionScope::delete tests ──────────────────────────────

    #[tokio::test]
    async fn delete_calls_endpoint() {
        let server = MockServer::start().await;
        let scope = make_scope(&server);

        Mock::given(method("DELETE"))
            .and(path("/v3/workspaces/ws1/conclusions/conc-42"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        scope.delete("conc-42").await.unwrap();
    }

    // ── F9.8.3: E2E lifecycle: create → list → query → delete ──────────

    #[tokio::test]
    async fn full_lifecycle_create_list_query_delete() {
        let server = MockServer::start().await;
        let scope = make_scope(&server);

        // Step 1: Create
        let create_body = serde_json::json!({
            "conclusions": [{
                "content": "likes rust",
                "observer_id": "alice",
                "observed_id": "bob",
            }]
        });
        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/conclusions"))
            .and(body_json(&create_body))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(vec![conclusion_json("likes rust", "c1")]),
            )
            .expect(1)
            .mount(&server)
            .await;

        let created = scope
            .create([ConclusionCreateParams::new("likes rust")])
            .await
            .unwrap();
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].id(), "c1");

        // Step 2: List
        let list_body = serde_json::json!({
            "filters": {
                "observer_id": "alice",
                "observed_id": "bob",
            }
        });
        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/conclusions/list"))
            .and(body_json(&list_body))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(page_json(vec![conclusion_json("likes rust", "c1")])),
            )
            .expect(1)
            .mount(&server)
            .await;

        let page = scope.list().send().await.unwrap();
        assert_eq!(page.total(), 1);

        // Step 3: Query
        let query_body = serde_json::json!({
            "query": "preferences",
            "top_k": 10,
            "filters": {
                "observer_id": "alice",
                "observed_id": "bob",
            }
        });
        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/conclusions/query"))
            .and(body_json(&query_body))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(vec![conclusion_json("likes rust", "c1")]),
            )
            .expect(1)
            .mount(&server)
            .await;

        let queried = scope.query("preferences").send().await.unwrap();
        assert_eq!(queried.len(), 1);
        assert_eq!(queried[0].id(), "c1");

        // Step 4: Delete
        Mock::given(method("DELETE"))
            .and(path("/v3/workspaces/ws1/conclusions/c1"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        scope.delete("c1").await.unwrap();
    }

    // ── PR4: partial-failure & validation gaps ───────────────────────────

    #[test]
    fn debug_appends_ellipsis_and_is_non_exhaustive() {
        let data = make_conclusion_data("b".repeat(80), None);
        let conc = Conclusion::from_parts("ws".to_owned(), data);
        let dbg = format!("{conc:?}");
        assert!(dbg.contains('…'), "expected ellipsis marker: {dbg}");
        assert!(
            dbg.contains(".."),
            "expected finish_non_exhaustive marker: {dbg}"
        );
    }

    #[tokio::test]
    async fn create_batch_over_100_partial_failure_first_chunk_committed() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let server = MockServer::start().await;
        let scope = make_scope(&server);

        let calls = std::sync::Arc::new(AtomicUsize::new(0));
        let calls_resp = calls.clone();

        // First chunk succeeds (committed server-side); every later request
        // fails. Documents the PR4 hazard: earlier-created conclusions are not
        // surfaced through the error.
        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/conclusions"))
            .respond_with(move |req: &wiremock::Request| {
                let n = calls_resp.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
                    let count = body["conclusions"].as_array().unwrap().len();
                    let items: Vec<serde_json::Value> = (0..count)
                        .map(|i| conclusion_json(&format!("c-{i}"), &format!("id-{i}")))
                        .collect();
                    ResponseTemplate::new(200).set_body_json(&items)
                } else {
                    ResponseTemplate::new(500).set_body_json(serde_json::json!({"detail": "boom"}))
                }
            })
            .mount(&server)
            .await;

        let params: Vec<ConclusionCreateParams> = (0..150)
            .map(|i| ConclusionCreateParams::new(format!("c-{i}")))
            .collect();
        let err = scope.create(params).await.unwrap_err();

        // The call fails as a whole (no typed partial-result yet — deferred).
        assert_eq!(err.code(), "server_error");
        // But the first chunk was already sent and committed server-side before
        // the failing one — proving the non-atomic, partial-failure hazard.
        assert!(
            calls.load(Ordering::SeqCst) >= 2,
            "first chunk should have been committed before the failing chunk"
        );
    }

    #[tokio::test]
    async fn query_top_k_error_names_top_k_not_search_top_k() {
        let scope =
            ConclusionScope::new(test_http(), "ws".to_owned(), "a".to_owned(), "b".to_owned());

        let err = scope.query("hobbies").top_k(0).send().await.unwrap_err();
        let HonchoError::Validation(msg) = err else {
            panic!("expected validation error");
        };
        assert!(msg.contains("top_k"), "message should name top_k: {msg}");
        assert!(
            !msg.contains("search_top_k"),
            "message must not leak the internal param name: {msg}"
        );
    }

    #[tokio::test]
    async fn query_rejects_whitespace_only() {
        let scope =
            ConclusionScope::new(test_http(), "ws".to_owned(), "a".to_owned(), "b".to_owned());

        let err = scope.query("   \t\n ").send().await.unwrap_err();
        assert!(matches!(err, HonchoError::Validation(_)));
        assert_eq!(err.code(), "validation_error");
    }
}
