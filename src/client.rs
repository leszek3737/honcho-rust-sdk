//! High-level Honcho SDK client.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use reqwest::header::HeaderMap;
use serde_json::Value;
use tokio::sync::OnceCell;
use url::Url;

use crate::error::{HonchoError, Result};
use crate::http::client::{DEFAULT_MAX_RETRIES, DEFAULT_TIMEOUT, HttpClient, normalize_base_url};
use crate::http::routes;
use crate::peer::Peer;
use crate::session::{PeerSpec, Session};
use crate::types::dream::QueueStatus;
use crate::types::message::MessageResponse;
use crate::types::peer::Peer as PeerResponse;
use crate::types::session::SessionResponse;
use crate::types::workspace::{Workspace, WorkspaceConfiguration};

/// Default `limit` for workspace search when the builder caller leaves it unset.
///
/// Single source of truth shared by the async [`Honcho::search`] builder and its
/// blocking mirror (`crate::blocking::Honcho::search`) so both surfaces stay in
/// lockstep instead of carrying independent magic literals.
pub(crate) const DEFAULT_SEARCH_LIMIT: u32 = 10;

/// API environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Environment {
    /// Local development server.
    Local,
    /// Production API at <https://api.honcho.dev>.
    #[default]
    Production,
}

impl Environment {
    fn base_url(self) -> &'static str {
        match self {
            Self::Local => "http://localhost:8000",
            Self::Production => "https://api.honcho.dev",
        }
    }
}

struct Inner {
    http: HttpClient,
    workspace_id: Arc<str>,
    base_url: Url,
    /// Single-flight "workspace exists" cache.
    ///
    /// The inner [`OnceCell`] provides single-flight initialization; wrapping it
    /// in a `Mutex<Arc<…>>` makes the cache *invalidatable* behind a shared
    /// reference (`&self`): [`Inner::reset_ensure`] swaps in a fresh cell so the
    /// next [`Honcho::ensure_workspace`] re-issues the request. The `Mutex`
    /// critical section only ever clones/replaces an `Arc`, so it is never held
    /// across an `.await`.
    ensure_workspace_once: Mutex<Arc<OnceCell<()>>>,
}

impl Inner {
    /// Snapshot the current ensure cell (cheap `Arc` clone under a short lock).
    fn ensure_cell(&self) -> Arc<OnceCell<()>> {
        self.ensure_workspace_once
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Invalidate the ensure cache so the next `ensure_workspace` re-issues
    /// the create request (e.g. after a server-side delete).
    fn reset_ensure(&self) {
        *self
            .ensure_workspace_once
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Arc::new(OnceCell::new());
    }
}

/// Entry point for the Honcho SDK.
///
/// Construct via [`Honcho::builder()`] followed by [`Honcho::from_params()`].
#[derive(Clone)]
pub struct Honcho {
    inner: Arc<Inner>,
}

/// Parameters for constructing a [`Honcho`] client.
///
/// Resolution order: explicit argument -> environment variable -> default.
#[derive(bon::Builder)]
#[builder(on(String, into))]
#[builder(finish_fn = build)]
pub struct HonchoParams {
    /// API key. Falls back to `HONCHO_API_KEY` env var.
    api_key: Option<String>,
    /// Base URL. Falls back to `HONCHO_URL` env var, then `HONCHO_API_URL`, then [`Environment::base_url`].
    base_url: Option<String>,
    /// API environment. Defaults to [`Environment::Production`].
    #[builder(default)]
    environment: Environment,
    /// Workspace ID. Falls back to `HONCHO_WORKSPACE_ID` env var, then "default".
    workspace_id: Option<String>,
    /// Custom `reqwest::Client`.
    http_client: Option<reqwest::Client>,
    /// Request timeout. Falls back to `HttpClient` default (60s).
    timeout: Option<Duration>,
    /// Max retries for transient errors. Falls back to `HttpClient` default (2).
    max_retries: Option<u32>,
    /// Extra default headers sent with every request.
    default_headers: Option<HeaderMap>,
    /// Extra default query parameters appended to every request.
    default_query: Option<Vec<(String, String)>>,
}

#[bon::bon]
impl Honcho {
    /// Quick constructor pointing at `base_url` for `workspace_id`.
    ///
    /// # Examples
    ///
    /// ```
    /// let client = honcho_ai::Honcho::new("http://localhost:8000", "my-workspace")?;
    /// # Ok::<(), honcho_ai::error::HonchoError>(())
    /// ```
    pub fn new(base_url: &str, workspace_id: &str) -> Result<Self> {
        validate_workspace_id(workspace_id)?;
        let url = normalize_base_url(base_url)?;
        let api_key = std::env::var("HONCHO_API_KEY")
            .ok()
            .filter(|s| !s.is_empty());
        let http = HttpClient::from_params_with_base_url(
            HttpClient::builder()
                .base_url(base_url.to_owned())
                .maybe_api_key(api_key)
                .build(),
            url.clone(),
        )?;
        Ok(Self {
            inner: Arc::new(Inner {
                http,
                workspace_id: Arc::from(workspace_id),
                base_url: url,
                ensure_workspace_once: Mutex::new(Arc::new(OnceCell::new())),
            }),
        })
    }

    /// Returns a builder for [`HonchoParams`].
    ///
    /// # Examples
    ///
    /// ```
    /// let params = honcho_ai::Honcho::builder()
    ///     .base_url("http://localhost:8000".to_owned())
    ///     .workspace_id("my-workspace".to_owned())
    ///     .build();
    /// let client = honcho_ai::Honcho::from_params(params)?;
    /// # Ok::<(), honcho_ai::error::HonchoError>(())
    /// ```
    pub fn builder() -> HonchoParamsBuilder {
        HonchoParams::builder()
    }

    /// Constructs a [`Honcho`] from params.
    ///
    /// # Examples
    ///
    /// ```
    /// let params = honcho_ai::Honcho::builder()
    ///     .base_url("http://localhost:8000".to_owned())
    ///     .build();
    /// let client = honcho_ai::Honcho::from_params(params)?;
    /// # Ok::<(), honcho_ai::error::HonchoError>(())
    /// ```
    pub fn from_params(params: HonchoParams) -> Result<Self> {
        let resolved_base_url = params
            .base_url
            .or_else(|| std::env::var("HONCHO_URL").ok().filter(|s| !s.is_empty()))
            .or_else(|| {
                std::env::var("HONCHO_API_URL")
                    .ok()
                    .filter(|s| !s.is_empty())
            })
            .unwrap_or_else(|| params.environment.base_url().to_owned());

        let resolved_api_key = params.api_key.or_else(|| {
            std::env::var("HONCHO_API_KEY")
                .ok()
                .filter(|s| !s.is_empty())
        });

        let resolved_workspace_id = params
            .workspace_id
            .or_else(|| {
                std::env::var("HONCHO_WORKSPACE_ID")
                    .ok()
                    .filter(|s| !s.is_empty())
            })
            .unwrap_or_else(|| "default".to_owned());

        validate_workspace_id(&resolved_workspace_id)?;
        let base_url = normalize_base_url(&resolved_base_url)?;

        let http = HttpClient::from_params_with_base_url(
            HttpClient::builder()
                .base_url(resolved_base_url)
                .maybe_api_key(resolved_api_key)
                .maybe_http_client(params.http_client)
                .timeout(params.timeout.unwrap_or(DEFAULT_TIMEOUT))
                .max_retries(params.max_retries.unwrap_or(DEFAULT_MAX_RETRIES))
                .default_headers(params.default_headers.unwrap_or_default())
                .default_query(params.default_query.unwrap_or_default())
                .build(),
            base_url.clone(),
        )?;

        Ok(Self {
            inner: Arc::new(Inner {
                http,
                workspace_id: Arc::from(resolved_workspace_id),
                base_url,
                ensure_workspace_once: Mutex::new(Arc::new(OnceCell::new())),
            }),
        })
    }

    /// Ensure the workspace exists on the server (`POST /v3/workspaces`).
    pub(crate) async fn ensure_workspace(&self) -> Result<()> {
        let cell = self.inner.ensure_cell();
        cell.get_or_try_init(|| async {
            let body = self.workspace_get_or_create_body();
            match self
                .inner
                .http
                .post::<_, Workspace>(&routes::workspaces(), Some(&body), &[])
                .await
            {
                Ok(_) => Ok(()),
                Err(e) if e.status_code() == Some(409) => Ok(()),
                Err(e) => Err(e),
            }
        })
        .await
        .map(drop)
    }

    /// Bypasses the workspace-ensure cache and always issues a
    /// `POST /v3/workspaces`, even if a prior ensure already succeeded.
    ///
    /// Use this to recover after a server-side workspace deletion. Repeated
    /// calls each hit the server.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example() -> honcho_ai::error::Result<()> {
    /// let client = honcho_ai::Honcho::new("http://localhost:8000", "ws-1")?;
    /// client.force_ensure().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn force_ensure(&self) -> Result<()> {
        // Bypass the cached `OnceCell`: invalidate it first so this call always
        // re-issues the ensure request, even when a prior lazy ensure already
        // succeeded or the workspace was deleted server-side.
        self.inner.reset_ensure();
        self.ensure_workspace().await
    }

    /// Returns the workspace ID this client is scoped to.
    ///
    /// # Examples
    ///
    /// ```
    /// let client = honcho_ai::Honcho::new("http://localhost:8000", "ws-1")?;
    /// assert_eq!(client.workspace_id(), "ws-1");
    /// # Ok::<(), honcho_ai::error::HonchoError>(())
    /// ```
    #[must_use]
    pub fn workspace_id(&self) -> &str {
        &self.inner.workspace_id
    }

    /// Returns the resolved base URL.
    #[must_use]
    pub fn base_url(&self) -> &Url {
        &self.inner.base_url
    }

    /// Returns the underlying HTTP client.
    pub(crate) fn http(&self) -> &HttpClient {
        &self.inner.http
    }

    /// Build the get-or-create body for this client's workspace.
    ///
    /// Reads go through `POST /v3/workspaces` because the server exposes no
    /// `GET /workspaces/{id}` (it answers `405`) — only `PUT`/`DELETE` are. The
    /// `WorkspaceCreate` skips its `None` fields, so the body is just `{"id": …}`
    /// and the call returns the workspace without mutating it.
    fn workspace_get_or_create_body(&self) -> crate::types::workspace::WorkspaceCreate {
        crate::types::workspace::WorkspaceCreate {
            id: self.inner.workspace_id.to_string(),
            metadata: None,
            configuration: None,
        }
    }

    /// POST the get-or-create workspace endpoint, returning the response as `T`
    /// (typed [`Workspace`] for reads, raw [`Value`] when unknown fields must be
    /// preserved).
    ///
    /// On success the workspace is guaranteed to exist — exactly what the lazy
    /// ensure-cache asserts — so the cache is populated here, letting a later
    /// `peer()`/`session()`/`search()` skip its otherwise-redundant ensure POST.
    async fn get_or_create_workspace<T: serde::de::DeserializeOwned + 'static>(&self) -> Result<T> {
        let body = self.workspace_get_or_create_body();
        let resp: T = self
            .inner
            .http
            .post(&routes::workspaces(), Some(&body), &[])
            .await?;
        let _ = self.inner.ensure_cell().set(());
        Ok(resp)
    }

    /// Fetch the workspace via the get-or-create `POST /v3/workspaces` endpoint.
    /// Doubles as the lazy workspace-ensure (see [`get_or_create_workspace`]).
    async fn fetch_workspace(&self) -> Result<Workspace> {
        self.get_or_create_workspace().await
    }

    /// Fetch workspace metadata from the server.
    pub async fn get_metadata(&self) -> Result<HashMap<String, Value>> {
        Ok(self.fetch_workspace().await?.metadata)
    }

    /// Set workspace metadata on the server.
    ///
    /// Unlike the read operations (e.g. [`get_metadata`](Self::get_metadata)),
    /// which lazily ensure the workspace exists, this write assumes the
    /// workspace already exists and fails with a 404 if it was deleted
    /// server-side.
    pub async fn set_metadata(&self, metadata: HashMap<String, Value>) -> Result<()> {
        let body = crate::types::workspace::WorkspaceMetadataSet { metadata };
        let _: Workspace = self
            .inner
            .http
            .put(&routes::workspace(self.workspace_id())?, Some(&body), &[])
            .await?;
        Ok(())
    }

    /// Fetch workspace configuration as a typed [`WorkspaceConfiguration`].
    ///
    /// # Example
    ///
    /// ```ignore
    /// let config = client.get_configuration().await?;
    /// if let Some(reasoning) = &config.reasoning {
    ///     println!("reasoning enabled: {:?}", reasoning.enabled);
    /// }
    /// ```
    pub async fn get_configuration(&self) -> Result<WorkspaceConfiguration> {
        Ok(self.fetch_workspace().await?.configuration)
    }

    /// Set workspace configuration from a typed [`WorkspaceConfiguration`].
    ///
    /// # Example
    ///
    /// ```no_run
    /// # async fn example() -> honcho_ai::error::Result<()> {
    /// use honcho_ai::types::common::ReasoningConfiguration;
    /// use honcho_ai::types::workspace::WorkspaceConfiguration;
    ///
    /// let client = honcho_ai::Honcho::new("http://localhost:8000", "ws-1")?;
    ///
    /// // Both types are `#[non_exhaustive]`, so build from `Default` and set
    /// // fields instead of using a struct literal / functional-update syntax.
    /// let mut reasoning = ReasoningConfiguration::default();
    /// reasoning.enabled = Some(true);
    ///
    /// let mut config = WorkspaceConfiguration::default();
    /// config.reasoning = Some(reasoning);
    ///
    /// client.set_configuration(&config).await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// Unlike the read operations (e.g.
    /// [`get_configuration`](Self::get_configuration)), which lazily ensure the
    /// workspace exists, this write assumes the workspace already exists and
    /// fails with a 404 if it was deleted server-side.
    pub async fn set_configuration(&self, config: &WorkspaceConfiguration) -> Result<()> {
        let body = crate::types::workspace::WorkspaceConfigurationSet {
            configuration: serde_json::to_value(config).map_err(|e| {
                HonchoError::Serialization {
                    path: "WorkspaceConfiguration".into(),
                    source: e,
                }
            })?,
        };
        let _: Workspace = self
            .inner
            .http
            .put(&routes::workspace(self.workspace_id())?, Some(&body), &[])
            .await?;
        Ok(())
    }

    /// Fetch workspace configuration as a raw JSON map.
    ///
    /// Prefer [`get_configuration`](Self::get_configuration) for typed access.
    /// Use this when the server returns fields not yet represented in
    /// [`WorkspaceConfiguration`].
    pub async fn get_configuration_raw(&self) -> Result<HashMap<String, Value>> {
        let raw: serde_json::Value = self.get_or_create_workspace().await?;
        // Take ownership of the parsed JSON and move the `configuration` object
        // out of it — no per-entry cloning.
        match raw {
            serde_json::Value::Object(mut map) => match map.remove("configuration") {
                Some(serde_json::Value::Object(configuration)) => {
                    Ok(configuration.into_iter().collect())
                }
                _ => Ok(HashMap::new()),
            },
            _ => Ok(HashMap::new()),
        }
    }

    /// Set workspace configuration from a raw JSON map.
    ///
    /// Prefer [`set_configuration`](Self::set_configuration) for typed access.
    /// Use this when you need to send fields not yet represented in
    /// [`WorkspaceConfiguration`].
    ///
    /// Unlike the read operations (e.g.
    /// [`get_configuration_raw`](Self::get_configuration_raw)), which lazily
    /// ensure the workspace exists, this write assumes the workspace already
    /// exists and fails with a 404 if it was deleted server-side.
    pub async fn set_configuration_raw(&self, configuration: HashMap<String, Value>) -> Result<()> {
        let body = crate::types::workspace::WorkspaceConfigurationSet {
            configuration: serde_json::Value::Object(configuration.into_iter().collect()),
        };
        let _: Workspace = self
            .inner
            .http
            .put(&routes::workspace(self.workspace_id())?, Some(&body), &[])
            .await?;
        Ok(())
    }

    /// Get or create a peer by ID.
    ///
    /// Returns a builder; finish with `.build().await`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example() -> honcho_ai::error::Result<()> {
    /// let client = honcho_ai::Honcho::new("http://localhost:8000", "ws-1")?;
    /// let peer = client.peer("alice").build().await?;
    /// # Ok(())
    /// # }
    /// ```
    #[builder(finish_fn = build, on(String, into))]
    pub async fn peer(
        &self,
        #[builder(start_fn)] id: String,
        metadata: Option<HashMap<String, Value>>,
        #[builder(name = config)] configuration: Option<HashMap<String, Value>>,
    ) -> Result<Peer> {
        if id.is_empty() {
            return Err(HonchoError::Configuration(
                "peer_id must not be empty".into(),
            ));
        }
        self.ensure_workspace().await?;
        let body = crate::types::peer::PeerCreate {
            id,
            metadata,
            configuration,
        };
        let resp: PeerResponse = self
            .inner
            .http
            .post(&routes::peers(&self.inner.workspace_id)?, Some(&body), &[])
            .await?;
        Peer::from_response(self, resp)
    }

    /// Get or create a session by ID.
    ///
    /// Returns a builder; finish with `.build().await`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example() -> honcho_ai::error::Result<()> {
    /// let client = honcho_ai::Honcho::new("http://localhost:8000", "ws-1")?;
    /// let session = client.session("s-42").build().await?;
    /// # Ok(())
    /// # }
    /// ```
    #[builder(finish_fn = build, on(String, into))]
    pub async fn session(
        &self,
        #[builder(start_fn)] id: String,
        metadata: Option<HashMap<String, Value>>,
        peers: Option<Vec<PeerSpec>>,
        configuration: Option<crate::SessionConfiguration>,
    ) -> Result<Session> {
        if id.is_empty() {
            return Err(HonchoError::Configuration(
                "session_id must not be empty".into(),
            ));
        }
        self.ensure_workspace().await?;
        let peers_map = peers.map(|specs| specs.into_iter().map(PeerSpec::into_parts).collect());
        let body = crate::types::session::SessionCreate {
            id,
            metadata,
            peers: peers_map,
            configuration,
        };
        let resp: SessionResponse = self
            .inner
            .http
            .post(
                &routes::sessions(&self.inner.workspace_id)?,
                Some(&body),
                &[],
            )
            .await?;
        Ok(Session::from_response(self, resp))
    }

    /// Refresh workspace state by re-fetching metadata and configuration.
    ///
    /// Issues a single get-or-create `POST /v3/workspaces` request (the server
    /// exposes no `GET /workspaces/{id}`); this also populates the lazy
    /// ensure-cache as a side effect.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example() -> honcho_ai::error::Result<()> {
    /// let client = honcho_ai::Honcho::new("http://localhost:8000", "ws-1")?;
    /// client.refresh().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn refresh(&self) -> Result<()> {
        self.fetch_workspace().await?;
        Ok(())
    }

    /// Search messages across the workspace.
    ///
    /// Returns a builder; finish with `.build().await`. `limit` defaults to 10.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example() -> honcho_ai::error::Result<()> {
    /// let client = honcho_ai::Honcho::new("http://localhost:8000", "ws-1")?;
    /// let results = client.search("important topic").build().await?;
    /// # Ok(())
    /// # }
    /// ```
    #[builder(finish_fn = build, on(String, into))]
    pub async fn search(
        &self,
        #[builder(start_fn)] query: String,
        #[builder(default = DEFAULT_SEARCH_LIMIT)] limit: u32,
        filters: Option<HashMap<String, Value>>,
    ) -> Result<Vec<crate::Message>> {
        self.ensure_workspace().await?;
        let body = crate::types::workspace::WorkspaceSearchRequest {
            query,
            limit,
            filters,
        };
        let responses: Vec<MessageResponse> = self
            .inner
            .http
            .post(
                &routes::workspace_search(&self.inner.workspace_id)?,
                Some(&body),
                &[],
            )
            .await?;
        // `Message::from_raw` no longer takes a workspace_id, so there is no
        // per-message `String` allocation here.
        Ok(responses
            .into_iter()
            .map(crate::Message::from_raw)
            .collect())
    }

    /// Get queue processing status.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example() -> honcho_ai::error::Result<()> {
    /// let client = honcho_ai::Honcho::new("http://localhost:8000", "ws-1")?;
    /// let status = client.queue_status(None, None, None).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn queue_status(
        &self,
        observer_id: Option<&str>,
        sender_id: Option<&str>,
        session_id: Option<&str>,
    ) -> Result<QueueStatus> {
        self.ensure_workspace().await?;
        let mut query: Vec<(&str, &str)> = Vec::new();
        if let Some(v) = observer_id {
            query.push(("observer_id", v));
        }
        if let Some(v) = sender_id {
            query.push(("sender_id", v));
        }
        if let Some(v) = session_id {
            query.push(("session_id", v));
        }
        self.inner
            .http
            .get(
                &routes::workspace_queue_status(&self.inner.workspace_id)?,
                &query,
            )
            .await
    }

    /// Schedule a dream task for memory consolidation.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example() -> honcho_ai::error::Result<()> {
    /// let client = honcho_ai::Honcho::new("http://localhost:8000", "ws-1")?;
    /// client.schedule_dream("alice", None, None).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn schedule_dream(
        &self,
        observer: &str,
        session_id: Option<&str>,
        observed_peer: Option<&str>,
    ) -> Result<()> {
        if observer.is_empty() {
            return Err(HonchoError::Validation(
                "observer must not be empty".to_string(),
            ));
        }
        self.ensure_workspace().await?;
        let observed_peer = observed_peer.unwrap_or(observer);
        let body = crate::types::dream::ScheduleDreamRequest {
            observer: observer.to_owned(),
            dream_type: crate::types::dream::DreamType::Omni,
            observed: Some(observed_peer.to_owned()),
            session_id: session_id.map(std::borrow::ToOwned::to_owned),
        };
        self.inner
            .http
            .post(
                &routes::workspace_schedule_dream(&self.inner.workspace_id)?,
                Some(&body),
                &[],
            )
            .await
    }

    /// Delete a workspace by ID.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example() -> honcho_ai::error::Result<()> {
    /// let client = honcho_ai::Honcho::new("http://localhost:8000", "ws-1")?;
    /// client.delete_workspace("old-ws").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn delete_workspace(&self, id: &str) -> Result<()> {
        self.inner
            .http
            .delete::<()>(&routes::workspace(id)?, &[])
            .await?;
        // If we just deleted the workspace this client is scoped to, invalidate
        // the ensure cache so later peer()/session() calls re-create it instead
        // of failing with 404 against a now-missing workspace.
        if id == self.workspace_id() {
            self.inner.reset_ensure();
        }
        Ok(())
    }

    // ── Paginated list methods (F4.5) ──────────────────────────────────

    /// Shared pagination plumbing for the peer/session list endpoints:
    /// ensure the workspace exists, then POST to `route` for one page.
    async fn list<T>(
        &self,
        route: &str,
        body: Option<&Value>,
        page: u64,
        size: u64,
        reverse: bool,
    ) -> Result<crate::types::pagination::Page<T>>
    where
        T: serde::de::DeserializeOwned + Clone + Send + 'static,
    {
        self.ensure_workspace().await?;
        crate::types::pagination::paginate_post(&self.inner.http, route, body, page, size, reverse)
            .await
    }

    /// List peers in the workspace. Returns a paginated result.
    ///
    /// Defaults: page=1, size=50, reverse=false, no filters.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example() -> honcho_ai::error::Result<()> {
    /// let client = honcho_ai::Honcho::new("http://localhost:8000", "ws-1")?;
    /// let page = client.peers().await?;
    /// for peer in page.items() {
    ///     println!("{}", peer.id);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn peers(&self) -> Result<crate::types::pagination::Page<crate::types::peer::Peer>> {
        self.list(
            &routes::peers_list(&self.inner.workspace_id)?,
            None,
            1,
            50,
            false,
        )
        .await
    }

    /// List peers with filters.
    ///
    /// `page` is 1-based. `size` must be in `1..=100`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example() -> honcho_ai::error::Result<()> {
    /// let client = honcho_ai::Honcho::new("http://localhost:8000", "ws-1")?;
    /// let mut filters = std::collections::HashMap::new();
    /// filters.insert("role".into(), "admin".into());
    /// let page = client.peers_with_filters(filters, 1, 10, false).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn peers_with_filters(
        &self,
        filters: HashMap<String, Value>,
        page: u64,
        size: u64,
        reverse: bool,
    ) -> Result<crate::types::pagination::Page<crate::types::peer::Peer>> {
        let body = crate::types::peer::PeerGet {
            filters: Some(filters),
        };
        let body_val = serde_json::to_value(&body).map_err(|e| HonchoError::Serialization {
            path: "PeerGet".into(),
            source: e,
        })?;
        self.list(
            &routes::peers_list(&self.inner.workspace_id)?,
            Some(&body_val),
            page,
            size,
            reverse,
        )
        .await
    }

    /// List sessions in the workspace. Returns a paginated result.
    ///
    /// Defaults: page=1, size=50, reverse=false, no filters.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example() -> honcho_ai::error::Result<()> {
    /// let client = honcho_ai::Honcho::new("http://localhost:8000", "ws-1")?;
    /// let page = client.sessions().await?;
    /// for session in page.items() {
    ///     println!("{}", session.id);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn sessions(
        &self,
    ) -> Result<crate::types::pagination::Page<crate::types::session::SessionResponse>> {
        self.list(
            &routes::sessions_list(&self.inner.workspace_id)?,
            None,
            1,
            50,
            false,
        )
        .await
    }

    /// List sessions with filters.
    ///
    /// `page` is 1-based. `size` must be in `1..=100`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example() -> honcho_ai::error::Result<()> {
    /// let client = honcho_ai::Honcho::new("http://localhost:8000", "ws-1")?;
    /// let mut filters = std::collections::HashMap::new();
    /// filters.insert("is_active".into(), true.into());
    /// let page = client.sessions_with_filters(filters, 1, 10, false).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn sessions_with_filters(
        &self,
        filters: HashMap<String, Value>,
        page: u64,
        size: u64,
        reverse: bool,
    ) -> Result<crate::types::pagination::Page<crate::types::session::SessionResponse>> {
        let body = crate::types::session::SessionGet {
            filters: Some(filters),
        };
        let body_val = serde_json::to_value(&body).map_err(|e| HonchoError::Serialization {
            path: "SessionGet".into(),
            source: e,
        })?;
        self.list(
            &routes::sessions_list(&self.inner.workspace_id)?,
            Some(&body_val),
            page,
            size,
            reverse,
        )
        .await
    }

    /// List workspace IDs. Returns a paginated result of ID strings.
    ///
    /// Defaults: page=1, size=50, reverse=false, no filters.
    /// No workspace scope required — queries all workspaces.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example() -> honcho_ai::error::Result<()> {
    /// let client = honcho_ai::Honcho::new("http://localhost:8000", "ws-1")?;
    /// let page = client.workspaces().await?;
    /// for id in page.items() {
    ///     println!("{id}");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn workspaces(&self) -> Result<crate::types::pagination::Page<Workspace, String>> {
        let page = crate::types::pagination::paginate_post::<Workspace>(
            &self.inner.http,
            &routes::workspaces_list(),
            None,
            1,
            50,
            false,
        )
        .await?;
        Ok(page.map(|ws| ws.id))
    }
}

fn validate_workspace_id(workspace_id: &str) -> Result<()> {
    if workspace_id.is_empty() {
        return Err(HonchoError::Configuration(
            "workspace_id must not be empty".into(),
        ));
    }

    if workspace_id.len() > 512 {
        return Err(HonchoError::Configuration(
            "workspace_id must be at most 512 characters".into(),
        ));
    }

    if !workspace_id
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        return Err(HonchoError::Configuration(
            "workspace_id must match [a-zA-Z0-9_-]+".into(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn workspace_body() -> serde_json::Value {
        serde_json::json!({ "id": "ws-1", "created_at": "2025-01-15T10:30:00Z" })
    }

    fn peer_body() -> serde_json::Value {
        serde_json::json!({
            "id": "p1",
            "workspace_id": "ws-1",
            "created_at": "2025-01-15T10:30:00Z",
        })
    }

    /// Build a wiremock-targeting client with `HONCHO_API_KEY` pinned unset.
    ///
    /// `Honcho::new` reads `HONCHO_API_KEY` from the process environment at
    /// construction time. These wiremock tests do not expect an auth header, so
    /// ambient env — or a concurrent env-mutating test (e.g.
    /// `new_picks_up_honcho_api_key_env`) — could otherwise leak a `Bearer`
    /// token into them. Pinning the var via `temp_env` makes construction
    /// deterministic regardless of ambient state, and `temp_env`'s internal lock
    /// serializes this read against the module's other env scopes without
    /// forcing the whole (async) test body to run serially.
    fn client_no_env_key(base_url: &str) -> Honcho {
        temp_env::with_var("HONCHO_API_KEY", None::<&str>, || {
            Honcho::new(base_url, "ws-1").unwrap()
        })
    }

    /// Count requests that hit the workspace-ensure endpoint (`POST /v3/workspaces`).
    async fn ensure_hits(server: &MockServer) -> usize {
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|r| r.url.path() == "/v3/workspaces")
            .count()
    }

    // Bug 1: `force_ensure` must bypass the cached OnceCell and re-issue the
    // ensure request every call, even after a prior success.
    #[tokio::test]
    async fn force_ensure_reissues_every_call() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v3/workspaces"))
            .respond_with(ResponseTemplate::new(200).set_body_json(workspace_body()))
            .mount(&server)
            .await;

        let client = client_no_env_key(&server.uri());
        client.force_ensure().await.unwrap();
        client.force_ensure().await.unwrap();

        let hits = ensure_hits(&server).await;
        assert!(
            hits >= 2,
            "force_ensure should re-issue PUT/POST, got {hits}"
        );
    }

    // Bug 2: `new` must honor `HONCHO_API_KEY` and send it as a bearer token.
    // The mock only matches when the Authorization header is present, so a
    // missing key would yield zero received requests.
    #[tokio::test]
    #[serial_test::serial]
    async fn new_picks_up_honcho_api_key_env() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v3/workspaces"))
            .and(header("authorization", "Bearer secret-key-123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(workspace_body()))
            .mount(&server)
            .await;

        let uri = server.uri();
        // `new` reads the env synchronously at construction time; build the
        // client inside the scoped env so the key is baked into the HttpClient.
        let client = temp_env::with_var("HONCHO_API_KEY", Some("secret-key-123"), || {
            Honcho::new(&uri, "ws-1").unwrap()
        });

        client.force_ensure().await.unwrap();
        assert_eq!(ensure_hits(&server).await, 1, "auth header was not sent");
    }

    // Bug 3: deleting the client's own workspace must invalidate the ensure
    // cache so a subsequent peer()/session() re-creates it (POST #2) instead of
    // short-circuiting and 404-ing.
    #[tokio::test]
    async fn delete_self_workspace_resets_ensure() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v3/workspaces"))
            .respond_with(ResponseTemplate::new(200).set_body_json(workspace_body()))
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/v3/workspaces/ws-1"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws-1/peers"))
            .respond_with(ResponseTemplate::new(200).set_body_json(peer_body()))
            .mount(&server)
            .await;

        let client = client_no_env_key(&server.uri());
        client.peer("alice").build().await.unwrap(); // ensure POST #1
        client.delete_workspace("ws-1").await.unwrap(); // resets cache
        client.peer("bob").build().await.unwrap(); // ensure POST #2

        let hits = ensure_hits(&server).await;
        assert!(
            hits >= 2,
            "deleting own workspace must force re-ensure, got {hits}"
        );
    }

    // Deleting a *different* workspace must NOT invalidate our ensure cache.
    #[tokio::test]
    async fn delete_other_workspace_keeps_ensure() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v3/workspaces"))
            .respond_with(ResponseTemplate::new(200).set_body_json(workspace_body()))
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/v3/workspaces/other-ws"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws-1/peers"))
            .respond_with(ResponseTemplate::new(200).set_body_json(peer_body()))
            .mount(&server)
            .await;

        let client = client_no_env_key(&server.uri());
        client.peer("alice").build().await.unwrap(); // ensure POST #1
        client.delete_workspace("other-ws").await.unwrap(); // unrelated, no reset
        client.peer("bob").build().await.unwrap(); // cache hit, no POST #2

        assert_eq!(
            ensure_hits(&server).await,
            1,
            "deleting an unrelated workspace must not reset the cache"
        );
    }

    // Bug 4: an empty `HONCHO_URL` must fall through to the next source
    // (here: the default environment URL), not be used as the base URL.
    #[test]
    #[serial_test::serial]
    fn empty_honcho_url_falls_back_to_default() {
        let client = temp_env::with_vars(
            [
                ("HONCHO_URL", Some("")),
                ("HONCHO_API_URL", None),
                ("HONCHO_WORKSPACE_ID", None),
                ("HONCHO_API_KEY", None),
            ],
            || Honcho::from_params(Honcho::builder().build()).unwrap(),
        );

        assert!(
            client
                .base_url()
                .as_str()
                .starts_with("https://api.honcho.dev"),
            "empty HONCHO_URL should fall back to the default, got {}",
            client.base_url()
        );
    }

    // The server exposes no `GET /v3/workspaces/{id}` (only PUT/DELETE), so
    // `refresh` reads through the get-or-create `POST /v3/workspaces` endpoint —
    // a single round-trip that both ensures and fetches the workspace.
    #[tokio::test]
    async fn refresh_uses_get_or_create() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v3/workspaces"))
            .respond_with(ResponseTemplate::new(200).set_body_json(workspace_body()))
            .mount(&server)
            .await;

        let client = client_no_env_key(&server.uri());
        client.refresh().await.unwrap();

        assert_eq!(
            ensure_hits(&server).await,
            1,
            "refresh must read the workspace via a single get-or-create POST"
        );
    }
}
