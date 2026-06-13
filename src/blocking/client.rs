use std::collections::HashMap;
use std::future::Future;

use serde_json::Value;
use url::Url;

use crate::client::HonchoParams;
use crate::error::Result;
use crate::session::PeerSpec;
use crate::types::dream::QueueStatus;
use crate::types::pagination::{Page, validate_pagination};
use crate::types::peer::Peer as PeerResponse;
use crate::types::session::Session as SessionResponse;
use crate::types::workspace::WorkspaceConfiguration;

use super::Peer as BlockingPeer;
use super::Session as BlockingSession;
use super::iter::collect_all_pages;
use super::runtime::block_on;

/// Synchronous wrapper around [`crate::Honcho`].
#[derive(Clone)]
pub struct Honcho {
    inner: crate::Honcho,
}

impl std::fmt::Debug for Honcho {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Honcho")
            .field("workspace_id", &self.inner.workspace_id())
            .field("base_url", &self.inner.base_url().as_str())
            .finish()
    }
}

impl Honcho {
    /// Drive an async page-producing future on the internal runtime and collect
    /// every page it seeds into a single `Vec`. Centralises the
    /// `block_on(async { fetch; collect_all_pages })` shape shared by all five
    /// paginated list methods below.
    ///
    /// Kept as a private associated function (rather than inlined per method)
    /// so the collect-all behaviour — and any future change to it — lives in a
    /// single place. The return-shape parity fix with the async client is
    /// tracked separately as a breaking change (PR6).
    fn collect_pages<TRaw, TOut>(
        first_page_fut: impl Future<Output = Result<Page<TRaw, TOut>>>,
    ) -> Result<Vec<TOut>>
    where
        TRaw: Clone + 'static,
        TOut: 'static,
    {
        // `block_on` wraps the future's output in a `Result` so the async-runtime
        // guard can surface its `Configuration` error; `?` unwraps that outer
        // layer, leaving the inner `collect_all_pages` result as the return value.
        block_on(async {
            let page = first_page_fut.await?;
            collect_all_pages(page).await
        })?
    }

    /// Create a blocking client pointed at `base_url` for `workspace_id`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let client = honcho_ai::blocking::Honcho::new("http://localhost:8000", "my-workspace")?;
    /// # Ok::<(), honcho_ai::error::HonchoError>(())
    /// ```
    pub fn new(base_url: &str, workspace_id: &str) -> Result<Self> {
        let inner = crate::Honcho::new(base_url, workspace_id)?;
        Ok(Self { inner })
    }

    /// Returns a builder for [`HonchoParams`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let params = honcho_ai::blocking::Honcho::builder()
    ///     .base_url("http://localhost:8000".to_owned())
    ///     .workspace_id("my-workspace".to_owned())
    ///     .build();
    /// let client = honcho_ai::blocking::Honcho::from_params(params)?;
    /// # Ok::<(), honcho_ai::error::HonchoError>(())
    /// ```
    pub fn builder() -> crate::client::HonchoParamsBuilder {
        crate::Honcho::builder()
    }

    /// Build from explicit params.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let params = honcho_ai::blocking::Honcho::builder()
    ///     .base_url("http://localhost:8000".to_owned())
    ///     .build();
    /// let client = honcho_ai::blocking::Honcho::from_params(params)?;
    /// # Ok::<(), honcho_ai::error::HonchoError>(())
    /// ```
    pub fn from_params(params: HonchoParams) -> Result<Self> {
        let inner = crate::Honcho::from_params(params)?;
        Ok(Self { inner })
    }

    /// Eagerly ensure the workspace exists on the server.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let client = honcho_ai::blocking::Honcho::new("http://localhost:8000", "ws-1")?;
    /// client.force_ensure()?;
    /// # Ok::<(), honcho_ai::error::HonchoError>(())
    /// ```
    pub fn force_ensure(&self) -> Result<()> {
        block_on(self.inner.force_ensure())?
    }

    /// Workspace ID this client is scoped to.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let client = honcho_ai::blocking::Honcho::new("http://localhost:8000", "ws-1")?;
    /// assert_eq!(client.workspace_id(), "ws-1");
    /// # Ok::<(), honcho_ai::error::HonchoError>(())
    /// ```
    #[must_use]
    pub fn workspace_id(&self) -> &str {
        self.inner.workspace_id()
    }

    /// Resolved base URL.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let client = honcho_ai::blocking::Honcho::new("http://localhost:8000", "ws-1")?;
    /// assert_eq!(client.base_url().as_str(), "http://localhost:8000/");
    /// # Ok::<(), honcho_ai::error::HonchoError>(())
    /// ```
    #[must_use]
    pub fn base_url(&self) -> &Url {
        self.inner.base_url()
    }

    /// Get or create a peer by ID.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let client = honcho_ai::blocking::Honcho::new("http://localhost:8000", "ws-1")?;
    /// let peer = client.peer("alice", None, None)?;
    /// # Ok::<(), honcho_ai::error::HonchoError>(())
    /// ```
    pub fn peer(
        &self,
        id: impl Into<String>,
        metadata: Option<HashMap<String, Value>>,
        configuration: Option<HashMap<String, Value>>,
    ) -> Result<BlockingPeer> {
        block_on(self.inner.peer(id, metadata, configuration))?.map(BlockingPeer::new)
    }

    /// Get or create a session by ID.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let client = honcho_ai::blocking::Honcho::new("http://localhost:8000", "ws-1")?;
    /// let session = client.session("s-42", None, None, None)?;
    /// # Ok::<(), honcho_ai::error::HonchoError>(())
    /// ```
    pub fn session(
        &self,
        id: impl Into<String>,
        metadata: Option<HashMap<String, Value>>,
        peers: Option<Vec<PeerSpec>>,
        configuration: Option<crate::SessionConfiguration>,
    ) -> Result<BlockingSession> {
        block_on(self.inner.session(id, metadata, peers, configuration))?.map(BlockingSession::new)
    }

    /// Search messages across the workspace.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let client = honcho_ai::blocking::Honcho::new("http://localhost:8000", "ws-1")?;
    /// let results = client.search("important topic", None, None)?;
    /// # Ok::<(), honcho_ai::error::HonchoError>(())
    /// ```
    pub fn search(
        &self,
        query: &str,
        limit: Option<u32>,
        filters: Option<HashMap<String, Value>>,
    ) -> Result<Vec<crate::Message>> {
        block_on(self.inner.search(query, limit, filters))?
    }

    /// Refresh workspace state.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let client = honcho_ai::blocking::Honcho::new("http://localhost:8000", "ws-1")?;
    /// client.refresh()?;
    /// # Ok::<(), honcho_ai::error::HonchoError>(())
    /// ```
    pub fn refresh(&self) -> Result<()> {
        block_on(self.inner.refresh())?
    }

    /// Get queue processing status.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let client = honcho_ai::blocking::Honcho::new("http://localhost:8000", "ws-1")?;
    /// let status = client.queue_status(None, None, None)?;
    /// # Ok::<(), honcho_ai::error::HonchoError>(())
    /// ```
    pub fn queue_status(
        &self,
        observer_id: Option<&str>,
        sender_id: Option<&str>,
        session_id: Option<&str>,
    ) -> Result<QueueStatus> {
        block_on(self.inner.queue_status(observer_id, sender_id, session_id))?
    }

    /// Schedule a dream task for memory consolidation.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let client = honcho_ai::blocking::Honcho::new("http://localhost:8000", "ws-1")?;
    /// client.schedule_dream("alice", None, None)?;
    /// # Ok::<(), honcho_ai::error::HonchoError>(())
    /// ```
    pub fn schedule_dream(
        &self,
        observer: &str,
        session_id: Option<&str>,
        observed_peer: Option<&str>,
    ) -> Result<()> {
        block_on(
            self.inner
                .schedule_dream(observer, session_id, observed_peer),
        )?
    }

    /// Delete a workspace by ID.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let client = honcho_ai::blocking::Honcho::new("http://localhost:8000", "ws-1")?;
    /// client.delete_workspace("old-ws")?;
    /// # Ok::<(), honcho_ai::error::HonchoError>(())
    /// ```
    ///
    /// # Warning
    ///
    /// Deleting the client's own workspace leaves this client pointing at a
    /// workspace ID that no longer exists on the server. The next lazy
    /// [`ensure_workspace`](Self::force_ensure) — triggered automatically by
    /// most other methods — will silently recreate an **empty** workspace with
    /// the same ID, so the deleted data is gone but the workspace reappears.
    /// Prefer a dedicated, short-lived client for destructive deletion.
    pub fn delete_workspace(&self, id: &str) -> Result<()> {
        block_on(self.inner.delete_workspace(id))?
    }

    /// Fetch workspace metadata.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let client = honcho_ai::blocking::Honcho::new("http://localhost:8000", "ws-1")?;
    /// let metadata = client.get_metadata()?;
    /// # Ok::<(), honcho_ai::error::HonchoError>(())
    /// ```
    pub fn get_metadata(&self) -> Result<HashMap<String, Value>> {
        block_on(self.inner.get_metadata())?
    }

    /// Set workspace metadata.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let client = honcho_ai::blocking::Honcho::new("http://localhost:8000", "ws-1")?;
    /// let mut metadata = std::collections::HashMap::new();
    /// metadata.insert("team".into(), "platform".into());
    /// client.set_metadata(metadata)?;
    /// # Ok::<(), honcho_ai::error::HonchoError>(())
    /// ```
    pub fn set_metadata(&self, metadata: HashMap<String, Value>) -> Result<()> {
        block_on(self.inner.set_metadata(metadata))?
    }

    /// Fetch workspace configuration as a typed [`WorkspaceConfiguration`].
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let client = honcho_ai::blocking::Honcho::new("http://localhost:8000", "ws-1")?;
    /// let config = client.get_configuration()?;
    /// if let Some(reasoning) = &config.reasoning {
    ///     println!("reasoning enabled: {:?}", reasoning.enabled);
    /// }
    /// ```
    pub fn get_configuration(&self) -> Result<WorkspaceConfiguration> {
        block_on(self.inner.get_configuration())?
    }

    /// Set workspace configuration from a typed [`WorkspaceConfiguration`].
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let client = honcho_ai::blocking::Honcho::new("http://localhost:8000", "ws-1")?;
    /// let config = WorkspaceConfiguration {
    ///     reasoning: Some(ReasoningConfiguration { enabled: Some(true), custom_instructions: None, ..Default::default() }),
    ///     ..Default::default()
    /// };
    /// client.set_configuration(&config)?;
    /// ```
    pub fn set_configuration(&self, config: &WorkspaceConfiguration) -> Result<()> {
        block_on(self.inner.set_configuration(config))?
    }

    /// Fetch workspace configuration as a raw JSON map.
    ///
    /// Prefer [`get_configuration`](Self::get_configuration) for typed access.
    /// Use this when the server returns fields not yet represented in
    /// [`WorkspaceConfiguration`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let client = honcho_ai::blocking::Honcho::new("http://localhost:8000", "ws-1")?;
    /// let raw = client.get_configuration_raw()?;
    /// # Ok::<(), honcho_ai::error::HonchoError>(())
    /// ```
    pub fn get_configuration_raw(&self) -> Result<HashMap<String, Value>> {
        block_on(self.inner.get_configuration_raw())?
    }

    /// Set workspace configuration from a raw JSON map.
    ///
    /// Prefer [`set_configuration`](Self::set_configuration) for typed access.
    /// Use this when you need to send fields not yet represented in
    /// [`WorkspaceConfiguration`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let client = honcho_ai::blocking::Honcho::new("http://localhost:8000", "ws-1")?;
    /// let mut raw = std::collections::HashMap::new();
    /// raw.insert("custom".into(), "value".into());
    /// client.set_configuration_raw(raw)?;
    /// # Ok::<(), honcho_ai::error::HonchoError>(())
    /// ```
    pub fn set_configuration_raw(&self, configuration: HashMap<String, Value>) -> Result<()> {
        block_on(self.inner.set_configuration_raw(configuration))?
    }

    /// List all peers in the workspace, collecting across pages.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let client = honcho_ai::blocking::Honcho::new("http://localhost:8000", "ws-1")?;
    /// let peers = client.peers()?;
    /// for peer in &peers {
    ///     println!("{}", peer.id);
    /// }
    /// # Ok::<(), honcho_ai::error::HonchoError>(())
    /// ```
    pub fn peers(&self) -> Result<Vec<PeerResponse>> {
        Self::collect_pages(self.inner.peers())
    }

    /// List peers with filters, collecting across pages.
    ///
    /// `page` is 1-based and `size` must be in `1..=100`; both are validated
    /// client-side before any network request and a violation returns
    /// [`HonchoError::Validation`](crate::error::HonchoError::Validation).
    ///
    /// # Warning (collect-all behaviour — to be unified in PR6)
    ///
    /// Unlike the async
    /// [`Honcho::peers_with_filters`](crate::Honcho::peers_with_filters), which
    /// returns a single [`Page`](crate::types::pagination::Page), this blocking
    /// method transparently fetches **all** pages starting from `page` until
    /// the end of the workspace (capped at 1000 page requests by
    /// `collect_all_pages`). The `page` and `size` parameters
    /// therefore only bound the **first** fetch; the returned `Vec` contains
    /// every matching peer from that page onward. If you only need one page,
    /// use the async client. The return-shape parity fix is a breaking change
    /// and is deferred to PR6.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let client = honcho_ai::blocking::Honcho::new("http://localhost:8000", "ws-1")?;
    /// let mut filters = std::collections::HashMap::new();
    /// filters.insert("role".into(), "admin".into());
    /// let peers = client.peers_with_filters(filters, 1, 10, false)?;
    /// # Ok::<(), honcho_ai::error::HonchoError>(())
    /// ```
    pub fn peers_with_filters(
        &self,
        filters: HashMap<String, Value>,
        page: u64,
        size: u64,
        reverse: bool,
    ) -> Result<Vec<PeerResponse>> {
        // Validate before entering the runtime so an out-of-range `page`/`size`
        // fails fast with a `Validation` error instead of first triggering a
        // lazy `ensure_workspace` network round-trip inside the async client.
        validate_pagination(page, size)?;
        Self::collect_pages(self.inner.peers_with_filters(filters, page, size, reverse))
    }

    /// List all sessions in the workspace, collecting across pages.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let client = honcho_ai::blocking::Honcho::new("http://localhost:8000", "ws-1")?;
    /// let sessions = client.sessions()?;
    /// for session in &sessions {
    ///     println!("{}", session.id);
    /// }
    /// # Ok::<(), honcho_ai::error::HonchoError>(())
    /// ```
    pub fn sessions(&self) -> Result<Vec<SessionResponse>> {
        Self::collect_pages(self.inner.sessions())
    }

    /// List sessions with filters, collecting across pages.
    ///
    /// `page` is 1-based and `size` must be in `1..=100`; both are validated
    /// client-side before any network request and a violation returns
    /// [`HonchoError::Validation`](crate::error::HonchoError::Validation).
    ///
    /// # Warning (collect-all behaviour — to be unified in PR6)
    ///
    /// Unlike the async
    /// [`Honcho::sessions_with_filters`](crate::Honcho::sessions_with_filters),
    /// which returns a single [`Page`](crate::types::pagination::Page), this
    /// blocking method transparently fetches **all** pages starting from
    /// `page` until the end of the workspace (capped at 1000 page requests by
    /// `collect_all_pages`). The `page` and `size` parameters
    /// therefore only bound the **first** fetch; the returned `Vec` contains
    /// every matching session from that page onward. If you only need one
    /// page, use the async client. The return-shape parity fix is a breaking
    /// change and is deferred to PR6.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let client = honcho_ai::blocking::Honcho::new("http://localhost:8000", "ws-1")?;
    /// let mut filters = std::collections::HashMap::new();
    /// filters.insert("is_active".into(), true.into());
    /// let sessions = client.sessions_with_filters(filters, 1, 10, false)?;
    /// # Ok::<(), honcho_ai::error::HonchoError>(())
    /// ```
    pub fn sessions_with_filters(
        &self,
        filters: HashMap<String, Value>,
        page: u64,
        size: u64,
        reverse: bool,
    ) -> Result<Vec<SessionResponse>> {
        // Validate before entering the runtime so an out-of-range `page`/`size`
        // fails fast with a `Validation` error instead of first triggering a
        // lazy `ensure_workspace` network round-trip inside the async client.
        validate_pagination(page, size)?;
        Self::collect_pages(
            self.inner
                .sessions_with_filters(filters, page, size, reverse),
        )
    }

    /// List all workspace IDs, collecting across pages.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let client = honcho_ai::blocking::Honcho::new("http://localhost:8000", "ws-1")?;
    /// let workspaces = client.workspaces()?;
    /// for id in &workspaces {
    ///     println!("{id}");
    /// }
    /// # Ok::<(), honcho_ai::error::HonchoError>(())
    /// ```
    pub fn workspaces(&self) -> Result<Vec<String>> {
        Self::collect_pages(self.inner.workspaces())
    }
}
