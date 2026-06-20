//! Session wrapper — construction, metadata, peer management, per-peer config.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};
use reqwest::Method;
use reqwest::multipart::Form;
use serde_json::Value;

use crate::error::{HonchoError, Result};
use crate::http::client::HttpClient;
use crate::http::routes;
use crate::message::Message;
use crate::types::message::MessageResponse;
use crate::types::session::SessionResponse;
use crate::types::session::{
    SessionConfiguration, SessionConfigurationSet, SessionPeerConfig, SessionUpdate,
};
use crate::upload::FileSource;

/// Single-lock cache of the server-owned session state.
///
/// Wrapping all mutable fields in one [`RwLock`] keeps them consistent: a
/// refresh/PUT response updates every field under one write, so readers never
/// observe a torn mix of fresh and stale fields.
#[derive(Default)]
struct SessionCacheState {
    metadata: Option<HashMap<String, Value>>,
    configuration: Option<SessionConfiguration>,
    is_active: bool,
}

pub(crate) struct SessionInner {
    http: HttpClient,
    // Stored as `Arc<str>` so per-Message/Peer/Session clones are refcount bumps
    // rather than allocations. Public method signatures keep taking `String`, so
    // boundary conversions still allocate; this only removes the internal churn.
    workspace_id: Arc<str>,
    id: String,
    cache: RwLock<SessionCacheState>,
    created_at: DateTime<Utc>,
}

impl SessionInner {
    /// Acquire the cache read lock, recovering from poisoning instead of panicking.
    fn read_lock(&self) -> std::sync::RwLockReadGuard<'_, SessionCacheState> {
        self.cache
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Acquire the cache write lock, recovering from poisoning instead of panicking.
    fn write_lock(&self) -> std::sync::RwLockWriteGuard<'_, SessionCacheState> {
        self.cache
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Atomically replace every cached field from a fresh server response.
    fn update_cache(&self, resp: &SessionResponse) {
        let mut cache = self.write_lock();
        cache.metadata = Some(resp.metadata.clone());
        cache.configuration = Some(resp.configuration.clone());
        cache.is_active = resp.is_active;
    }
}

/// A session in a Honcho workspace.
///
/// Wraps the API response and provides methods for metadata, configuration,
/// peer management, messages, and more.
#[derive(Clone)]
pub struct Session {
    inner: Arc<SessionInner>,
}

/// Specification for adding/setting peers on a session.
///
/// Use [`PeerSpec::Id`] for a bare peer ID or [`PeerSpec::WithConfig`] to
/// include per-peer observation settings.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum PeerSpec {
    /// A peer identified by ID with no explicit config.
    Id(String),
    /// A peer identified by ID with per-session configuration.
    WithConfig(String, SessionPeerConfig),
}

impl From<&str> for PeerSpec {
    fn from(s: &str) -> Self {
        Self::Id(s.to_owned())
    }
}

impl From<String> for PeerSpec {
    fn from(s: String) -> Self {
        Self::Id(s)
    }
}

impl From<&crate::Peer> for PeerSpec {
    fn from(p: &crate::Peer) -> Self {
        Self::Id(p.id().to_owned())
    }
}

impl From<crate::Peer> for PeerSpec {
    fn from(p: crate::Peer) -> Self {
        Self::Id(p.id().to_owned())
    }
}

impl From<(String, SessionPeerConfig)> for PeerSpec {
    fn from((id, cfg): (String, SessionPeerConfig)) -> Self {
        Self::WithConfig(id, cfg)
    }
}

impl From<(&str, SessionPeerConfig)> for PeerSpec {
    fn from((id, cfg): (&str, SessionPeerConfig)) -> Self {
        Self::WithConfig(id.to_owned(), cfg)
    }
}

impl From<(&crate::Peer, SessionPeerConfig)> for PeerSpec {
    fn from((p, cfg): (&crate::Peer, SessionPeerConfig)) -> Self {
        Self::WithConfig(p.id().to_owned(), cfg)
    }
}

impl PeerSpec {
    /// Decompose into `(peer_id, config)`.
    ///
    /// The bare-ID variant ([`PeerSpec::Id`]) yields a default
    /// [`SessionPeerConfig`] (all observation settings unset), so callers can
    /// treat both variants uniformly without re-matching.
    #[must_use]
    pub fn into_parts(self) -> (String, SessionPeerConfig) {
        match self {
            Self::Id(id) => (id, SessionPeerConfig::default()),
            Self::WithConfig(id, cfg) => (id, cfg),
        }
    }
}

/// Builder for the file-upload operation returned by [`Session::upload_file`].
///
/// Call `.peer(id)` (required) and optionally chain `.metadata()`,
/// `.configuration()`, `.created_at()` before calling `.send()`.
#[must_use]
pub struct UploadFileBuilder<'a> {
    session: &'a Session,
    source: Option<FileSource>,
    peer_id: Option<String>,
    metadata: Option<Value>,
    configuration: Option<Value>,
    created_at: Option<DateTime<Utc>>,
}

fn serialize_upload_fields(
    builder: &UploadFileBuilder<'_>,
) -> Result<impl Fn(Form) -> Form + Clone + Send + 'static> {
    let metadata_text = builder
        .metadata
        .as_ref()
        .map(|md| {
            serde_json::to_string(md).map_err(|e| HonchoError::Serialization {
                path: "MessageUploadFormMetadata".into(),
                source: e,
            })
        })
        .transpose()?;

    let configuration_text = builder
        .configuration
        .as_ref()
        .map(|cfg| {
            serde_json::to_string(cfg).map_err(|e| HonchoError::Serialization {
                path: "MessageUploadFormConfiguration".into(),
                source: e,
            })
        })
        .transpose()?;

    let created_at_text = builder.created_at.map(|dt| dt.to_rfc3339());

    Ok(move |mut form: Form| -> Form {
        if let Some(ref md) = metadata_text {
            form = form.text("metadata", md.clone());
        }
        if let Some(ref cfg) = configuration_text {
            form = form.text("configuration", cfg.clone());
        }
        if let Some(ref dt) = created_at_text {
            form = form.text("created_at", dt.clone());
        }
        form
    })
}

/// Derive the multipart filename for a `Path` upload, rejecting paths with no
/// final file-name component.
///
/// A bare root (`/`) or a trailing `..` has no file name; uploading it would
/// send an empty/absent filename. Surfacing it as [`HonchoError::Validation`]
/// here lets the production upload path fail fast before any request is made.
fn derive_path_filename(path: &std::path::Path) -> Result<String> {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .ok_or_else(|| {
            HonchoError::Validation(format!(
                "file source path has no file name component: {}",
                path.display()
            ))
        })
}

/// Build a multipart form for an in-memory (`Bytes`) upload payload.
///
/// The payload is a [`bytes::Bytes`], so each retry clones it as a cheap
/// refcount bump rather than copying the buffer. An invalid `content_type`
/// is surfaced as [`HonchoError::Validation`] instead of being silently dropped.
fn build_form(
    filename: String,
    bytes: bytes::Bytes,
    content_type: &str,
    peer_id: String,
    add_text_fields: impl Fn(Form) -> Form,
) -> Result<Form> {
    let mut headers = reqwest::header::HeaderMap::new();
    let value = reqwest::header::HeaderValue::from_str(content_type)
        .map_err(|_| HonchoError::Validation("invalid content_type".into()))?;
    headers.insert(reqwest::header::CONTENT_TYPE, value);

    let file_part = reqwest::multipart::Part::stream(reqwest::Body::from(bytes))
        .file_name(filename)
        .headers(headers);
    let form = Form::new().part("file", file_part).text("peer_id", peer_id);
    Ok(add_text_fields(form))
}

impl UploadFileBuilder<'_> {
    /// Set the peer that owns the uploaded file (required).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(session: &honcho_ai::Session) {
    /// let _builder = session.upload_file(honcho_ai::FileSource::bytes("f.txt", b"data", "text/plain")).peer("alice");
    /// # }
    /// ```
    pub fn peer(mut self, id: impl Into<String>) -> Self {
        self.peer_id = Some(id.into());
        self
    }

    /// Attach arbitrary JSON metadata to the created message(s).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(session: &honcho_ai::Session) {
    /// let _builder = session.upload_file(honcho_ai::FileSource::bytes("f.txt", b"data", "text/plain"))
    ///     .peer("alice")
    ///     .metadata(serde_json::json!({"source": "upload"}));
    /// # }
    /// ```
    pub fn metadata(mut self, value: Value) -> Self {
        self.metadata = Some(value);
        self
    }

    /// Attach configuration to the created message(s).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(session: &honcho_ai::Session) {
    /// let _builder = session.upload_file(honcho_ai::FileSource::bytes("f.txt", b"data", "text/plain"))
    ///     .peer("alice")
    ///     .configuration(serde_json::json!({"reasoning": true}));
    /// # }
    /// ```
    pub fn configuration(mut self, value: Value) -> Self {
        self.configuration = Some(value);
        self
    }

    /// Override the creation timestamp (RFC 3339).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(session: &honcho_ai::Session) {
    /// let _builder = session.upload_file(honcho_ai::FileSource::bytes("f.txt", b"data", "text/plain"))
    ///     .peer("alice")
    ///     .created_at(chrono::Utc::now());
    /// # }
    /// ```
    pub fn created_at(mut self, dt: DateTime<Utc>) -> Self {
        self.created_at = Some(dt);
        self
    }

    /// Resolve the file source, build the multipart form, POST, and return
    /// the created messages.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(session: &honcho_ai::Session) -> honcho_ai::error::Result<()> {
    /// let msgs = session
    ///     .upload_file(honcho_ai::FileSource::bytes("doc.pdf", b"data", "application/pdf"))
    ///     .peer("alice")
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError::Validation`] if no peer was set via `.peer()`.
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(skip(self), name = "upload_file_send")
    )]
    pub async fn send(self) -> Result<Vec<crate::Message>> {
        // Local items are hoisted to the top of the scope (items-after-statements).
        // `Resolved` normalizes the source to a single in-memory representation so
        // the form-building match has only two arms; `FormFactory` is the per-retry
        // form builder (see the usage sites below for the rationale).
        enum Resolved {
            Path {
                path: std::path::PathBuf,
                filename: String,
            },
            Bytes {
                filename: String,
                bytes: bytes::Bytes,
                content_type: String,
            },
        }
        type FormFactory = Box<
            dyn Fn() -> std::pin::Pin<
                    Box<dyn std::future::Future<Output = Result<Form>> + Send + 'static>,
                > + Send
                + 'static,
        >;

        let add_text_fields = serialize_upload_fields(&self)?;

        let Some(peer_id) = self.peer_id else {
            return Err(HonchoError::Validation("peer_id is required".into()));
        };
        let Some(source) = self.source else {
            return Err(HonchoError::Validation("file source is required".into()));
        };

        // Normalize the source to a single in-memory representation: `Stream` is
        // buffered into `Bytes` here so the form-building match has only two arms
        // (disk-streamed `Path` vs. in-memory `Bytes`). The `Bytes` payload makes
        // per-retry clones cheap refcount bumps instead of full buffer copies.
        let resolved = match source {
            FileSource::Path(path) => {
                // Derive (and validate) the multipart filename up front so an
                // unnamed path fails fast before any request is attempted.
                let filename = derive_path_filename(&path)?;
                Resolved::Path { path, filename }
            }
            FileSource::Bytes {
                filename,
                bytes,
                content_type,
            } => Resolved::Bytes {
                filename,
                // `FileSource::Bytes` carries a `Vec<u8>` (kept non-breaking in the
                // public API). Convert it to `bytes::Bytes` exactly once here, before
                // the retry closure is built, so each retry clones a cheap refcount
                // handle (`Bytes::clone`) instead of copying the whole buffer.
                bytes: bytes::Bytes::from(bytes),
                content_type,
            },
            FileSource::Stream {
                filename,
                mut reader,
                content_type,
            } => {
                let mut buf = Vec::new();
                tokio::io::AsyncReadExt::read_to_end(&mut reader, &mut buf)
                    .await
                    .map_err(HonchoError::from)?;
                Resolved::Bytes {
                    filename,
                    bytes: bytes::Bytes::from(buf),
                    content_type,
                }
            }
        };

        // FileSource::Path — Part::file() streams from disk, re-opens on retry.
        // Resolved::Bytes — in-memory payload, cheap to clone per retry.
        let form_factory: FormFactory = match resolved {
            Resolved::Path { path, filename } => Box::new(move || {
                let path = path.clone();
                let filename = filename.clone();
                let peer_id = peer_id.clone();
                let add_text_fields = add_text_fields.clone();
                Box::pin(async move {
                    // `Part::file` streams the body straight from disk (re-opened
                    // on each retry) — the file is never buffered into memory.
                    let file_part = reqwest::multipart::Part::file(&path)
                        .await
                        .map_err(HonchoError::from)?
                        .file_name(filename);
                    let form = Form::new().part("file", file_part).text("peer_id", peer_id);
                    Ok(add_text_fields(form))
                })
            }),
            Resolved::Bytes {
                filename,
                bytes,
                content_type,
            } => Box::new(move || {
                let filename = filename.clone();
                let bytes = bytes.clone();
                let content_type = content_type.clone();
                let peer_id = peer_id.clone();
                let add_text_fields = add_text_fields.clone();
                Box::pin(async move {
                    build_form(filename, bytes, &content_type, peer_id, add_text_fields)
                })
            }),
        };

        let route =
            routes::messages_upload(&self.session.inner.workspace_id, &self.session.inner.id)?;

        let responses: Vec<MessageResponse> = self
            .session
            .inner
            .http
            .post_multipart(&route, form_factory, &[])
            .await?;

        Ok(responses
            .into_iter()
            .map(crate::Message::from_raw)
            .collect())
    }
}

impl Session {
    pub(crate) fn from_parts(
        http: HttpClient,
        workspace_id: String,
        resp: SessionResponse,
    ) -> Self {
        Self {
            inner: Arc::new(SessionInner {
                http,
                workspace_id: Arc::from(workspace_id),
                id: resp.id,
                cache: RwLock::new(SessionCacheState {
                    metadata: Some(resp.metadata),
                    configuration: Some(resp.configuration),
                    is_active: resp.is_active,
                }),
                created_at: resp.created_at,
            }),
        }
    }

    pub(crate) fn from_response(honcho: &crate::Honcho, resp: SessionResponse) -> Self {
        Self::from_parts(
            honcho.http().clone(),
            honcho.workspace_id().to_owned(),
            resp,
        )
    }

    /// The session's unique identifier.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(session: &honcho_ai::Session) {
    /// println!("{}", session.id());
    /// # }
    /// ```
    #[must_use]
    pub fn id(&self) -> &str {
        &self.inner.id
    }

    /// Whether the session is currently active.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(session: &honcho_ai::Session) {
    /// if session.is_active() {
    ///     println!("session is active");
    /// }
    /// # }
    /// ```
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.inner.read_lock().is_active
    }

    /// Cached metadata from the last API response.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(session: &honcho_ai::Session) {
    /// if let Some(meta) = session.metadata() {
    ///     println!("{meta:?}");
    /// }
    /// # }
    /// ```
    #[must_use]
    pub fn metadata(&self) -> Option<HashMap<String, Value>> {
        self.inner.read_lock().metadata.clone()
    }

    /// Cached configuration from the last API response.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(session: &honcho_ai::Session) {
    /// if let Some(config) = session.configuration() {
    ///     println!("{config:?}");
    /// }
    /// # }
    /// ```
    #[must_use]
    pub fn configuration(&self) -> Option<SessionConfiguration> {
        self.inner.read_lock().configuration.clone()
    }

    /// When the session was created.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(session: &honcho_ai::Session) {
    /// println!("{}", session.created_at());
    /// # }
    /// ```
    #[must_use]
    pub fn created_at(&self) -> DateTime<Utc> {
        self.inner.created_at
    }

    // ── F6.1: Refresh / Metadata / Configuration CRUD ──────────────────

    /// Refresh the session's cached metadata and configuration from the server.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(session: &honcho_ai::Session) -> honcho_ai::error::Result<()> {
    /// session.refresh().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn refresh(&self) -> Result<()> {
        self.refresh_into().await?;
        Ok(())
    }

    /// Re-fetch the current session state, refresh the cache, and return the
    /// response.
    ///
    /// The server exposes no `GET /sessions/{id}` (it answers `405`), so this
    /// reads through the get-or-create `POST /sessions` collection endpoint.
    /// One consequence: a session deleted server-side is silently re-created
    /// rather than surfacing as `NotFound`. Callers that need the fresh metadata
    /// or configuration should read it from the returned response to avoid a
    /// refresh-then-read race against concurrent writers.
    async fn refresh_into(&self) -> Result<SessionResponse> {
        // The individual-resource path (`GET /v3/workspaces/{ws}/sessions/{id}`)
        // is not exposed by the server — only `PUT`/`DELETE` are — so reads go
        // through the collection get-or-create, which returns the full session
        // without mutating it: `SessionCreate` skips its `None` fields, so the
        // request body is just `{"id": …}`.
        let body = crate::types::session::SessionCreate {
            id: self.inner.id.clone(),
            metadata: None,
            peers: None,
            configuration: None,
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
        self.inner.update_cache(&resp);
        Ok(resp)
    }

    /// Fetch and return the session's metadata, updating the cache.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(session: &honcho_ai::Session) -> honcho_ai::error::Result<()> {
    /// let meta = session.get_metadata().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_metadata(&self) -> Result<HashMap<String, Value>> {
        let resp = self.refresh_into().await?;
        Ok(resp.metadata)
    }

    /// Set session metadata on the server and update the cache.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(session: &honcho_ai::Session) -> honcho_ai::error::Result<()> {
    /// let mut meta = std::collections::HashMap::new();
    /// meta.insert("topic".into(), "rust".into());
    /// session.set_metadata(meta).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn set_metadata(&self, metadata: HashMap<String, Value>) -> Result<()> {
        let body = crate::types::session::SessionMetadataSet { metadata };
        let resp: SessionResponse = self
            .inner
            .http
            .put(
                &routes::session(&self.inner.workspace_id, &self.inner.id)?,
                Some(&body),
                &[],
            )
            .await?;
        self.inner.update_cache(&resp);
        Ok(())
    }

    /// Fetch and return session configuration, updating the cache.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(session: &honcho_ai::Session) -> honcho_ai::error::Result<()> {
    /// let config = session.get_configuration().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_configuration(&self) -> Result<SessionConfiguration> {
        let resp = self.refresh_into().await?;
        Ok(resp.configuration)
    }

    /// Set session configuration on the server and update the cache.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(session: &honcho_ai::Session) -> honcho_ai::error::Result<()> {
    /// use honcho_ai::types::session::SessionConfiguration;
    /// let config = SessionConfiguration::default();
    /// session.set_configuration(&config).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn set_configuration(&self, configuration: &SessionConfiguration) -> Result<()> {
        let body = SessionUpdate {
            metadata: None,
            configuration: Some(configuration.clone()),
        };
        let resp: SessionResponse = self
            .inner
            .http
            .put(
                &routes::session(&self.inner.workspace_id, &self.inner.id)?,
                Some(&body),
                &[],
            )
            .await?;
        self.inner.update_cache(&resp);
        Ok(())
    }

    /// Fetch session configuration as a raw JSON map.
    ///
    /// Prefer [`get_configuration`](Self::get_configuration) for typed access.
    /// Use this when the server returns fields not yet represented in
    /// [`SessionConfiguration`].
    pub async fn get_configuration_raw(&self) -> Result<HashMap<String, Value>> {
        let body = crate::types::session::SessionCreate {
            id: self.inner.id.clone(),
            metadata: None,
            peers: None,
            configuration: None,
        };
        let raw: serde_json::Value = self
            .inner
            .http
            .post(
                &routes::sessions(&self.inner.workspace_id)?,
                Some(&body),
                &[],
            )
            .await?;
        match raw.get("configuration") {
            Some(serde_json::Value::Object(map)) => {
                Ok(map.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            }
            _ => Ok(HashMap::new()),
        }
    }

    /// Set session configuration from a raw JSON map.
    ///
    /// Prefer [`set_configuration`](Self::set_configuration) for typed access.
    /// Use this when you need to send fields not yet represented in
    /// [`SessionConfiguration`].
    pub async fn set_configuration_raw(&self, configuration: HashMap<String, Value>) -> Result<()> {
        let body = SessionConfigurationSet { configuration };
        let resp: SessionResponse = self
            .inner
            .http
            .put(
                &routes::session(&self.inner.workspace_id, &self.inner.id)?,
                Some(&body),
                &[],
            )
            .await?;
        self.inner.update_cache(&resp);
        Ok(())
    }

    // ── F6.2: Peer Management ──────────────────────────────────────────

    /// Add a single peer to this session.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(session: &honcho_ai::Session) -> honcho_ai::error::Result<()> {
    /// session.add_peer("alice").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn add_peer(&self, id: impl Into<String>) -> Result<()> {
        self.add_peers(std::iter::once(PeerSpec::Id(id.into())))
            .await
    }

    /// Add multiple peers to this session.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(session: &honcho_ai::Session) -> honcho_ai::error::Result<()> {
    /// session.add_peers(["alice", "bob"]).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn add_peers(
        &self,
        specs: impl IntoIterator<Item = impl Into<PeerSpec>>,
    ) -> Result<()> {
        let peers_map = normalize_peers(specs)?;
        let route = routes::session_peers(&self.inner.workspace_id, &self.inner.id)?;
        self.inner.http.post(&route, Some(&peers_map), &[]).await
    }

    /// Set the complete peer list for this session (replaces existing).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(session: &honcho_ai::Session) -> honcho_ai::error::Result<()> {
    /// session.set_peers(["alice", "bob"]).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn set_peers(
        &self,
        specs: impl IntoIterator<Item = impl Into<PeerSpec>>,
    ) -> Result<()> {
        let peers_map = normalize_peers(specs)?;
        let route = routes::session_peers(&self.inner.workspace_id, &self.inner.id)?;
        self.inner.http.put(&route, Some(&peers_map), &[]).await
    }

    /// Remove peers from this session.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(session: &honcho_ai::Session) -> honcho_ai::error::Result<()> {
    /// session.remove_peers(["bob"]).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn remove_peers(
        &self,
        ids: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<()> {
        let id_list: Vec<String> = ids.into_iter().map(Into::into).collect();
        let route = routes::session_peers(&self.inner.workspace_id, &self.inner.id)?;
        self.inner
            .http
            .request::<_, ()>(Method::DELETE, &route, Some(&id_list), &[])
            .await
    }

    /// List peers in this session.
    ///
    /// This call is **all-or-nothing**: it walks every page and deserializes each
    /// peer. If any single peer fails to deserialize (`Peer::from_parts` errors),
    /// the whole call returns that error and the peers already accumulated from
    /// earlier pages are discarded — partial results are never returned.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(session: &honcho_ai::Session) -> honcho_ai::error::Result<()> {
    /// let peers = session.peers().await?;
    /// for p in &peers {
    ///     println!("{}", p.id());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn peers(&self) -> Result<Vec<crate::Peer>> {
        use crate::types::pagination::PageResponse;

        let route = routes::session_peers(&self.inner.workspace_id, &self.inner.id)?;
        let mut all = Vec::new();
        let mut page: u64 = 1;
        loop {
            let page_str = page.to_string();
            let resp: PageResponse<crate::types::peer::Peer> = self
                .inner
                .http
                .get(&route, &[("page", page_str.as_str())])
                .await?;
            let total_pages = resp.pages;
            let was_empty = resp.items.is_empty();
            for item in resp.items {
                all.push(crate::Peer::from_parts(
                    self.inner.http.clone(),
                    self.inner.workspace_id.to_string(),
                    item,
                )?);
            }
            // Stop once we have walked every page. `was_empty` guards against a
            // server reporting a page count larger than the items it returns,
            // which would otherwise loop forever.
            if was_empty || page >= total_pages {
                break;
            }
            page += 1;
        }
        Ok(all)
    }

    // ── F6.3: Per-peer configuration ───────────────────────────────────

    /// Get per-peer configuration for a specific peer in this session.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(session: &honcho_ai::Session) -> honcho_ai::error::Result<()> {
    /// let config = session.get_peer_configuration("alice").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_peer_configuration(&self, peer_id: &str) -> Result<SessionPeerConfig> {
        let route = routes::session_peer_config(&self.inner.workspace_id, &self.inner.id, peer_id)?;
        self.inner.http.get(&route, &[]).await
    }

    /// Set per-peer configuration for a specific peer in this session.
    ///
    /// The peer must already be present in the session. This method does not
    /// create or add peers; use [`Session::add_peer`] or [`Session::add_peers`]
    /// first. If the peer is absent, the server may return 404/`NotFound`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # async fn example(session: &honcho_ai::Session) -> honcho_ai::error::Result<()> {
    /// use honcho_ai::types::session::SessionPeerConfig;
    /// let config = SessionPeerConfig { observe_me: Some(true), observe_others: Some(false) };
    /// session.set_peer_configuration("alice", &config).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn set_peer_configuration(
        &self,
        peer_id: &str,
        config: &SessionPeerConfig,
    ) -> Result<()> {
        let route = routes::session_peer_config(&self.inner.workspace_id, &self.inner.id, peer_id)?;
        self.inner.http.put(&route, Some(config), &[]).await
    }

    // ── F6.4: Messages ─────────────────────────────────────────────────

    /// Add messages to this session.
    ///
    /// If more than 100 messages are provided, they are automatically chunked
    /// into batches of 100 and sent as separate requests. On chunk failure the
    /// already-sent messages are **not** rolled back (non-atomic). When a chunk
    /// fails after earlier chunks succeeded, the error is a
    /// [`HonchoError::PartialFailure`] containing the successfully created
    /// messages from the earlier chunks.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(client: &honcho_ai::Honcho, session: &honcho_ai::Session) -> honcho_ai::error::Result<()> {
    /// let peer = client.peer("alice").build().await?;
    /// let msg = peer.message("Hello!").build()?;
    /// let messages = session.add_messages(vec![msg]).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn add_messages(
        &self,
        messages: Vec<crate::types::message::MessageCreate>,
    ) -> Result<Vec<Message>> {
        if messages.is_empty() {
            return Ok(Vec::new());
        }

        let route = routes::messages(&self.inner.workspace_id, &self.inner.id)?;

        let responses: Vec<MessageResponse> = if messages.len() <= 100 {
            let body = crate::types::message::MessageBatchCreate { messages };
            self.inner.http.post(&route, Some(&body), &[]).await?
        } else {
            let mut all = Vec::with_capacity(messages.len());
            // Drain owned messages 100 at a time instead of cloning each chunk:
            // `by_ref().take(100)` moves each batch out of the iterator.
            let mut iter = messages.into_iter();
            loop {
                let batch: Vec<crate::types::message::MessageCreate> =
                    iter.by_ref().take(100).collect();
                if batch.is_empty() {
                    break;
                }
                let body = crate::types::message::MessageBatchCreate { messages: batch };
                match self
                    .inner
                    .http
                    .post::<_, Vec<MessageResponse>>(&route, Some(&body), &[])
                    .await
                {
                    Ok(batch_responses) => all.extend(batch_responses),
                    Err(e) if all.is_empty() => return Err(e),
                    Err(e) => {
                        let sent = all.len();
                        let partial: Vec<Message> =
                            all.into_iter().map(Message::from_raw).collect();
                        return Err(HonchoError::PartialFailure {
                            messages: partial,
                            sent,
                            error: Box::new(e),
                        });
                    }
                }
            }
            all
        };

        Ok(responses.into_iter().map(Message::from_raw).collect())
    }

    /// List messages in this session with default pagination (no filters, page 1, size 50).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(session: &honcho_ai::Session) -> honcho_ai::error::Result<()> {
    /// let page = session.messages().await?;
    /// for msg in page.items() {
    ///     println!("{}", msg.content());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn messages(
        &self,
    ) -> Result<crate::types::pagination::Page<MessageResponse, Message>> {
        self.messages_with_options(None, 1, 50, false).await
    }

    /// List messages in this session with optional filters, page, size, and reverse.
    ///
    /// `page` is 1-based. `size` must be in `1..=100`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(session: &honcho_ai::Session) -> honcho_ai::error::Result<()> {
    /// let page = session.messages_with_options(None, 1, 25, false).await?;
    /// for msg in page.items() {
    ///     println!("{}", msg.content());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn messages_with_options(
        &self,
        filters: Option<HashMap<String, Value>>,
        page: u64,
        size: u64,
        reverse: bool,
    ) -> Result<crate::types::pagination::Page<MessageResponse, Message>> {
        let route = routes::messages_list(&self.inner.workspace_id, &self.inner.id)?;
        let body = filters
            .map(|f| {
                serde_json::to_value(f).map_err(|e| HonchoError::Serialization {
                    path: "MessageGet".into(),
                    source: e,
                })
            })
            .transpose()?;
        let result: crate::types::pagination::Page<MessageResponse> =
            crate::types::pagination::paginate_post(
                &self.inner.http,
                &route,
                body.as_ref(),
                page,
                size,
                reverse,
            )
            .await?;
        Ok(result.map(Message::from_raw))
    }

    // ── F7.3: File upload ───────────────────────────────────────────────

    /// Begin a file upload to this session.
    ///
    /// The API currently accepts `text/plain`, `application/pdf`, and
    /// `application/json`; other MIME types may be rejected by the server.
    ///
    /// Returns an [`UploadFileBuilder`]. You **must** call `.peer(id)` and
    /// then `.send()` to complete the upload.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let messages = session
    ///     .upload_file(FileSource::bytes("doc.pdf", data, "application/pdf"))
    ///     .peer("alice")
    ///     .send()
    ///     .await?;
    /// ```
    pub fn upload_file(&self, source: impl Into<FileSource>) -> UploadFileBuilder<'_> {
        UploadFileBuilder {
            session: self,
            source: Some(source.into()),
            peer_id: None,
            metadata: None,
            configuration: None,
            created_at: None,
        }
    }

    /// Begin a file upload to this session from a streaming reader.
    ///
    /// The API currently accepts `text/plain`, `application/pdf`, and
    /// `application/json`; other MIME types may be rejected by the server.
    ///
    /// The reader is fully buffered into memory before uploading. This is
    /// **not** true streaming — use [`Session::upload_file`] with a
    /// [`FileSource::path`] for filesystem streaming that avoids buffering.
    ///
    /// Returns an [`UploadFileBuilder`]. You **must** call `.peer(id)` and
    /// then `.send()` to complete the upload.
    pub fn upload_file_streamed(
        &self,
        filename: impl Into<String>,
        reader: impl tokio::io::AsyncRead + Send + 'static,
        content_type: impl Into<String>,
    ) -> UploadFileBuilder<'_> {
        UploadFileBuilder {
            session: self,
            source: Some(FileSource::stream(filename, reader, content_type)),
            peer_id: None,
            metadata: None,
            configuration: None,
            created_at: None,
        }
    }

    // ── F6.5: Delete, clone, get/update message ────────────────────────

    /// Delete this session.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(session: &honcho_ai::Session) -> honcho_ai::error::Result<()> {
    /// session.delete().await?;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self), fields(session_id = self.inner.id.as_str())))]
    pub async fn delete(&self) -> Result<()> {
        self.inner
            .http
            .delete(
                &routes::session(&self.inner.workspace_id, &self.inner.id)?,
                &[],
            )
            .await
    }

    /// Clone this session, returning a new `Session`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(session: &honcho_ai::Session) -> honcho_ai::error::Result<()> {
    /// let cloned = session.clone_session().await?;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self), fields(session_id = self.inner.id.as_str())))]
    pub async fn clone_session(&self) -> Result<Session> {
        let route = routes::session_clone(&self.inner.workspace_id, &self.inner.id)?;
        let resp: SessionResponse = self.inner.http.post(&route, None::<&Value>, &[]).await?;
        Ok(Self::from_parts(
            self.inner.http.clone(),
            self.inner.workspace_id.to_string(),
            resp,
        ))
    }

    /// Clone this session up to (and including) the given message.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(session: &honcho_ai::Session) -> honcho_ai::error::Result<()> {
    /// let cloned = session.clone_session_with_message("msg-42").await?;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self), fields(session_id = self.inner.id.as_str())))]
    pub async fn clone_session_with_message(&self, message_id: &str) -> Result<Session> {
        let route = routes::session_clone(&self.inner.workspace_id, &self.inner.id)?;
        let resp: SessionResponse = self
            .inner
            .http
            .post(&route, None::<&Value>, &[("message_id", message_id)])
            .await?;
        Ok(Self::from_parts(
            self.inner.http.clone(),
            self.inner.workspace_id.to_string(),
            resp,
        ))
    }

    /// Get a single message by ID.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(session: &honcho_ai::Session) -> honcho_ai::error::Result<()> {
    /// let msg = session.get_message("msg-1").await?;
    /// println!("{}", msg.content());
    /// # Ok(())
    /// # }
    /// ```
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self), fields(session_id = self.inner.id.as_str())))]
    pub async fn get_message(&self, id: &str) -> Result<Message> {
        let route = routes::message(&self.inner.workspace_id, &self.inner.id, id)?;
        let resp: MessageResponse = self.inner.http.get(&route, &[]).await?;
        Ok(Message::from_raw(resp))
    }

    /// Update a message's metadata.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(session: &honcho_ai::Session) -> honcho_ai::error::Result<()> {
    /// let mut meta = std::collections::HashMap::new();
    /// meta.insert("edited".into(), true.into());
    /// let msg = session.update_message("msg-1", meta).await?;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self, metadata), fields(session_id = self.inner.id.as_str())))]
    pub async fn update_message(
        &self,
        id: &str,
        metadata: HashMap<String, Value>,
    ) -> Result<Message> {
        let route = routes::message(&self.inner.workspace_id, &self.inner.id, id)?;
        let body = crate::types::message::MessageMetadataSet { metadata };
        let resp: MessageResponse = self.inner.http.put(&route, Some(&body), &[]).await?;
        Ok(Message::from_raw(resp))
    }

    // ── F6.6: Context ───────────────────────────────────────────────────

    /// Get the session context with default parameters.
    ///
    /// Fetches messages, summary, peer representation, and peer card for this session.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(session: &honcho_ai::Session) -> honcho_ai::error::Result<()> {
    /// let ctx = session.context().await?;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self), fields(session_id = self.inner.id.as_str())))]
    pub async fn context(&self) -> Result<crate::types::session::SessionContext> {
        self.context_builder().send().await
    }

    /// Get the session context with custom parameters.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(session: &honcho_ai::Session) -> honcho_ai::error::Result<()> {
    /// use honcho_ai::types::session::SessionContextOptions;
    /// let opts = SessionContextOptions::builder().summary(true).build();
    /// let ctx = session.context_with_options(&opts).await?;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self), fields(session_id = self.inner.id.as_str())))]
    pub async fn context_with_options(
        &self,
        options: &crate::types::session::SessionContextOptions,
    ) -> Result<crate::types::session::SessionContext> {
        fetch_session_context(
            &self.inner.http,
            &self.inner.workspace_id,
            &self.inner.id,
            options,
        )
        .await
    }

    /// Get a context builder for fine-grained control over session context parameters.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(session: &honcho_ai::Session) -> honcho_ai::error::Result<()> {
    /// let ctx = session.context_builder()
    ///     .summary(true)
    ///     .peer_target("alice")
    ///     .search_query("preferences")
    ///     .search_top_k(10)
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn context_builder(&self) -> SessionContextBuilder {
        SessionContextBuilder {
            http: self.inner.http.clone(),
            workspace_id: self.inner.workspace_id.to_string(),
            session_id: self.inner.id.clone(),
            summary: true,
            limit_to_session: false,
            tokens: None,
            peer_target: None,
            peer_perspective: None,
            search_query: None,
            search_top_k: None,
            search_max_distance: None,
            include_most_frequent: None,
            max_conclusions: None,
        }
    }

    // ── F6.8: Summaries ─────────────────────────────────────────────────

    /// Get available summaries for this session.
    ///
    /// Returns both short and long summaries if they are available.
    /// Summaries are created asynchronously as messages are added.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(session: &honcho_ai::Session) -> honcho_ai::error::Result<()> {
    /// let summaries = session.summaries().await?;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self), fields(session_id = self.inner.id.as_str())))]
    pub async fn summaries(&self) -> Result<crate::types::session::SessionSummaries> {
        let route = routes::session_summaries(&self.inner.workspace_id, &self.inner.id)?;
        self.inner.http.get(&route, &[]).await
    }

    // ── F6.9: Search, representation, queue_status ──────────────────────

    /// Search messages within this session (default limit of 10).
    ///
    /// Returns `Err(HonchoError::Validation)` when `query` is empty.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(session: &honcho_ai::Session) -> honcho_ai::error::Result<()> {
    /// let results = session.search("important topic").await?;
    /// for msg in results {
    ///     println!("{}", msg.content());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self), fields(session_id = self.inner.id.as_str())))]
    pub async fn search(&self, query: &str) -> Result<Vec<Message>> {
        self.search_with_options(&crate::types::message::MessageSearchOptions {
            query: query.to_string(),
            filters: None,
            limit: 10,
        })
        .await
    }

    /// Search messages within this session with custom options (limit, filters).
    ///
    /// Returns `Err(HonchoError::Validation)` when `query` is empty.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(session: &honcho_ai::Session) -> honcho_ai::error::Result<()> {
    /// use honcho_ai::types::message::MessageSearchOptions;
    /// let opts = MessageSearchOptions::builder().query("topic").limit(20).build();
    /// let results = session.search_with_options(&opts).await?;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self, options), fields(session_id = self.inner.id.as_str())))]
    pub async fn search_with_options(
        &self,
        options: &crate::types::message::MessageSearchOptions,
    ) -> Result<Vec<Message>> {
        if options.query.is_empty() {
            return Err(crate::error::HonchoError::Validation(
                "query must not be empty".to_string(),
            ));
        }
        let route = routes::session_search(&self.inner.workspace_id, &self.inner.id)?;
        let responses: Vec<MessageResponse> =
            self.inner.http.post(&route, Some(&options), &[]).await?;
        Ok(responses.into_iter().map(Message::from_raw).collect())
    }

    /// Get a peer's representation scoped to this session.
    ///
    /// Uses the peer representation endpoint with `session_id` filter.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(session: &honcho_ai::Session) -> honcho_ai::error::Result<()> {
    /// let rep = session.representation("alice").await?;
    /// println!("{rep}");
    /// # Ok(())
    /// # }
    /// ```
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self), fields(session_id = self.inner.id.as_str())))]
    pub async fn representation(&self, peer_id: &str) -> Result<String> {
        self.representation_builder(peer_id).send().await
    }

    /// Create a builder for fine-grained representation requests scoped to this session.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(session: &honcho_ai::Session) -> honcho_ai::error::Result<()> {
    /// let rep = session.representation_builder("alice")
    ///     .search_query("hobbies")
    ///     .search_top_k(10)
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn representation_builder(
        &self,
        peer_id: impl Into<String>,
    ) -> SessionRepresentationBuilder {
        SessionRepresentationBuilder {
            http: self.inner.http.clone(),
            workspace_id: self.inner.workspace_id.to_string(),
            session_id: self.inner.id.clone(),
            peer_id: peer_id.into(),
            target: None,
            search_query: None,
            search_top_k: None,
            search_max_distance: None,
            include_most_frequent: None,
            max_conclusions: None,
        }
    }

    /// Get the processing queue status for this session.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(session: &honcho_ai::Session) -> honcho_ai::error::Result<()> {
    /// let status = session.queue_status(None, None).await?;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self), fields(session_id = self.inner.id.as_str())))]
    pub async fn queue_status(
        &self,
        observer_id: Option<&str>,
        sender_id: Option<&str>,
    ) -> Result<crate::types::dream::QueueStatus> {
        let route = routes::workspace_queue_status(&self.inner.workspace_id)?;
        let mut query: Vec<(&str, &str)> = vec![("session_id", self.inner.id.as_str())];
        if let Some(v) = observer_id {
            query.push(("observer_id", v));
        }
        if let Some(v) = sender_id {
            query.push(("sender_id", v));
        }
        self.inner.http.get(&route, &query).await
    }
}

/// Builder for fine-grained representation requests scoped to a session.
#[must_use]
pub struct SessionRepresentationBuilder {
    http: HttpClient,
    workspace_id: String,
    session_id: String,
    peer_id: String,
    target: Option<String>,
    search_query: Option<String>,
    search_top_k: Option<u32>,
    search_max_distance: Option<f64>,
    include_most_frequent: Option<bool>,
    max_conclusions: Option<u32>,
}

impl SessionRepresentationBuilder {
    /// Get the representation for a specific target peer.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(session: &honcho_ai::Session) {
    /// let _builder = session.representation_builder("alice").target("bob");
    /// # }
    /// ```
    pub fn target(mut self, val: impl Into<String>) -> Self {
        self.target = Some(val.into());
        self
    }

    /// Semantic search query to curate the representation.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(session: &honcho_ai::Session) {
    /// let _builder = session.representation_builder("alice").search_query("hobbies");
    /// # }
    /// ```
    pub fn search_query(mut self, val: impl Into<String>) -> Self {
        self.search_query = Some(val.into());
        self
    }

    /// Number of semantic-search-retrieved conclusions (1–100).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(session: &honcho_ai::Session) {
    /// let _builder = session.representation_builder("alice").search_top_k(20);
    /// # }
    /// ```
    pub fn search_top_k(mut self, val: u32) -> Self {
        self.search_top_k = Some(val);
        self
    }

    /// Maximum distance for semantically relevant conclusions (0.0–1.0).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(session: &honcho_ai::Session) {
    /// let _builder = session.representation_builder("alice").search_max_distance(0.5);
    /// # }
    /// ```
    pub fn search_max_distance(mut self, val: f64) -> Self {
        self.search_max_distance = Some(val);
        self
    }

    /// Whether to include the most frequent conclusions.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(session: &honcho_ai::Session) {
    /// let _builder = session.representation_builder("alice").include_most_frequent(true);
    /// # }
    /// ```
    pub fn include_most_frequent(mut self, val: bool) -> Self {
        self.include_most_frequent = Some(val);
        self
    }

    /// Maximum number of conclusions to include (1–100).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(session: &honcho_ai::Session) {
    /// let _builder = session.representation_builder("alice").max_conclusions(25);
    /// # }
    /// ```
    pub fn max_conclusions(mut self, val: u32) -> Self {
        self.max_conclusions = Some(val);
        self
    }

    /// Send the representation request with the configured parameters.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(session: &honcho_ai::Session) -> honcho_ai::error::Result<()> {
    /// let rep = session.representation_builder("alice")
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
    /// Returns `HonchoError::Validation` if `search_top_k`, `search_max_distance`,
    /// or `max_conclusions` are out of range.
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self), fields(session_id = self.session_id.as_str(), peer_id = self.peer_id.as_str())))]
    pub async fn send(self) -> Result<String> {
        crate::types::session::validate_search_params(
            self.search_top_k,
            self.search_max_distance,
            self.max_conclusions,
        )?;

        let params = crate::types::peer::PeerRepresentationGet {
            session_id: Some(self.session_id),
            target: self.target,
            search_query: self.search_query,
            search_top_k: self.search_top_k,
            search_max_distance: self.search_max_distance,
            include_most_frequent: self.include_most_frequent,
            max_conclusions: self.max_conclusions,
        };

        let route = routes::peer_representation(&self.workspace_id, &self.peer_id)?;
        let resp: crate::types::dialectic::RepresentationResponse =
            self.http.post(&route, Some(&params), &[]).await?;
        Ok(resp.representation)
    }
}

/// Builder for fine-grained session context requests.
///
/// Created via [`Session::context_builder()`].
///
/// # Examples
///
/// ```no_run
/// # async fn example(session: &honcho_ai::Session) -> honcho_ai::error::Result<()> {
/// let ctx = session.context_builder()
///     .summary(true)
///     .peer_target("alice")
///     .search_query("preferences")
///     .search_top_k(10)
///     .send()
///     .await?;
/// # Ok(())
/// # }
/// ```
#[must_use]
pub struct SessionContextBuilder {
    http: HttpClient,
    workspace_id: String,
    session_id: String,
    summary: bool,
    limit_to_session: bool,
    tokens: Option<u32>,
    peer_target: Option<String>,
    peer_perspective: Option<String>,
    search_query: Option<String>,
    search_top_k: Option<u32>,
    search_max_distance: Option<f64>,
    include_most_frequent: Option<bool>,
    max_conclusions: Option<u32>,
}

impl SessionContextBuilder {
    /// Whether to include summaries (default: `true`).
    pub fn summary(mut self, val: bool) -> Self {
        self.summary = val;
        self
    }

    /// Limit context to this session only (default: `false`).
    pub fn limit_to_session(mut self, val: bool) -> Self {
        self.limit_to_session = val;
        self
    }

    /// Maximum number of tokens for the context.
    pub fn tokens(mut self, val: u32) -> Self {
        self.tokens = Some(val);
        self
    }

    /// Target peer for perspective-based context.
    pub fn peer_target(mut self, val: impl Into<String>) -> Self {
        self.peer_target = Some(val.into());
        self
    }

    /// Perspective peer for viewing context.
    pub fn peer_perspective(mut self, val: impl Into<String>) -> Self {
        self.peer_perspective = Some(val.into());
        self
    }

    /// Semantic search query to filter relevant conclusions.
    pub fn search_query(mut self, val: impl Into<String>) -> Self {
        self.search_query = Some(val.into());
        self
    }

    /// Number of semantic-search-retrieved conclusions (1–100).
    pub fn search_top_k(mut self, val: u32) -> Self {
        self.search_top_k = Some(val);
        self
    }

    /// Maximum distance for semantically relevant conclusions (0.0–1.0).
    pub fn search_max_distance(mut self, val: f64) -> Self {
        self.search_max_distance = Some(val);
        self
    }

    /// Whether to include the most frequent conclusions.
    pub fn include_most_frequent(mut self, val: bool) -> Self {
        self.include_most_frequent = Some(val);
        self
    }

    /// Maximum number of conclusions to include (1–100).
    pub fn max_conclusions(mut self, val: u32) -> Self {
        self.max_conclusions = Some(val);
        self
    }

    /// Send the context request with the configured parameters.
    ///
    /// # Errors
    ///
    /// Returns `HonchoError::Validation` if `search_top_k`, `search_max_distance`,
    /// or `max_conclusions` are out of range, if `peer_perspective` is set without
    /// `peer_target`, or if `search_query` is set without `peer_target`.
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self), fields(session_id = self.session_id.as_str())))]
    pub async fn send(self) -> Result<crate::types::session::SessionContext> {
        let options = crate::types::session::SessionContextOptions {
            summary: self.summary,
            limit_to_session: self.limit_to_session,
            tokens: self.tokens,
            peer_target: self.peer_target,
            peer_perspective: self.peer_perspective,
            search_query: self.search_query,
            search_top_k: self.search_top_k,
            search_max_distance: self.search_max_distance,
            include_most_frequent: self.include_most_frequent,
            max_conclusions: self.max_conclusions,
        };
        fetch_session_context(&self.http, &self.workspace_id, &self.session_id, &options).await
    }
}

/// Validate options, then GET the session context endpoint with the query
/// parameters they produce. Shared by [`Session::context_with_options`] and
/// [`SessionContextBuilder::send`] so the route/query/GET logic lives once.
async fn fetch_session_context(
    http: &HttpClient,
    workspace_id: &str,
    session_id: &str,
    options: &crate::types::session::SessionContextOptions,
) -> Result<crate::types::session::SessionContext> {
    options.validate()?;
    let route = routes::session_context(workspace_id, session_id)?;
    let params = options.to_query_params();
    let refs: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, &**v)).collect();
    http.get(&route, &refs).await
}

fn normalize_peers(
    specs: impl IntoIterator<Item = impl Into<PeerSpec>>,
) -> Result<serde_json::Value> {
    use serde_json::map::Entry;

    let mut map = serde_json::Map::new();
    for s in specs {
        // Decompose by value: the id is owned (no extra clone) and the config is
        // taken directly, defaulting for the bare-ID variant.
        let (id, cfg) = s.into().into_parts();
        let val = serde_json::to_value(&cfg).map_err(|e| HonchoError::Serialization {
            path: "SessionPeerConfig".into(),
            source: e,
        })?;
        // Reject duplicate IDs instead of letting a later entry silently clobber
        // an earlier one. The Entry API does a single lookup for both the
        // duplicate check and the insert.
        match map.entry(id) {
            Entry::Occupied(e) => {
                return Err(HonchoError::Validation(format!(
                    "duplicate peer id: {}",
                    e.key()
                )));
            }
            Entry::Vacant(e) => {
                e.insert(val);
            }
        }
    }
    Ok(Value::Object(map))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use static_assertions::assert_impl_all;

    use super::*;
    use crate::http::client::HttpClient;
    use crate::types::session::SessionResponse;
    use chrono::TimeZone;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    assert_impl_all!(UploadFileBuilder<'_>: Send);

    fn session_json(id: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "workspace_id": "ws1",
            "is_active": true,
            "metadata": {},
            "configuration": {},
            "created_at": "2025-01-15T10:30:00Z"
        })
    }

    fn message_response_json(content: &str, peer_id: &str) -> serde_json::Value {
        serde_json::json!({
            "id": "msg_1",
            "content": content,
            "peer_id": peer_id,
            "session_id": "sess1",
            "metadata": {},
            "created_at": "2025-01-15T10:30:00Z",
            "workspace_id": "ws1",
            "token_count": 5
        })
    }

    fn make_session(http: HttpClient, id: &str) -> Session {
        let resp: SessionResponse = serde_json::from_value(session_json(id)).unwrap();
        Session::from_parts(http, "ws1".to_owned(), resp)
    }

    fn upload_response_json() -> serde_json::Value {
        serde_json::json!([message_response_json("extracted text", "alice")])
    }

    #[tokio::test]
    async fn upload_file_with_bytes_sends_correct_multipart() {
        let server = MockServer::start().await;
        let http =
            HttpClient::from_params(HttpClient::builder().base_url(server.uri()).build()).unwrap();
        let session = make_session(http, "sess1");

        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/sessions/sess1/messages/upload"))
            .and(body_string_contains("file content here"))
            .and(body_string_contains("peer_id"))
            .and(body_string_contains("alice"))
            .respond_with(ResponseTemplate::new(200).set_body_json(upload_response_json()))
            .mount(&server)
            .await;

        let msgs = session
            .upload_file(FileSource::bytes(
                "test.txt",
                b"file content here".as_slice(),
                "text/plain",
            ))
            .peer("alice")
            .send()
            .await
            .unwrap();

        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content(), "extracted text");
        assert_eq!(msgs[0].peer_id(), "alice");
    }

    #[tokio::test]
    async fn upload_file_with_metadata_sends_json_stringified_field() {
        let server = MockServer::start().await;
        let http =
            HttpClient::from_params(HttpClient::builder().base_url(server.uri()).build()).unwrap();
        let session = make_session(http, "sess1");

        let metadata = serde_json::json!({"source": "upload", "priority": 1});

        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/sessions/sess1/messages/upload"))
            .and(body_string_contains("\"source\":\"upload\""))
            .and(body_string_contains("\"priority\":1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(upload_response_json()))
            .mount(&server)
            .await;

        let msgs = session
            .upload_file(FileSource::bytes("f.txt", b"data", "text/plain"))
            .peer("alice")
            .metadata(metadata)
            .send()
            .await
            .unwrap();

        assert_eq!(msgs.len(), 1);
    }

    #[tokio::test]
    async fn upload_file_with_configuration_sends_json_stringified() {
        let server = MockServer::start().await;
        let http =
            HttpClient::from_params(HttpClient::builder().base_url(server.uri()).build()).unwrap();
        let session = make_session(http, "sess1");

        let config = serde_json::json!({"reasoning": {"enabled": true}});

        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/sessions/sess1/messages/upload"))
            .and(body_string_contains("\"reasoning\""))
            .and(body_string_contains("\"enabled\":true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(upload_response_json()))
            .mount(&server)
            .await;

        let msgs = session
            .upload_file(FileSource::bytes("f.txt", b"data", "text/plain"))
            .peer("bob")
            .configuration(config)
            .send()
            .await
            .unwrap();

        assert_eq!(msgs.len(), 1);
    }

    #[tokio::test]
    async fn upload_file_with_created_at_datetime_sends_iso_string() {
        let server = MockServer::start().await;
        let http =
            HttpClient::from_params(HttpClient::builder().base_url(server.uri()).build()).unwrap();
        let session = make_session(http, "sess1");

        let dt = Utc.with_ymd_and_hms(2025, 3, 14, 9, 26, 53).unwrap();

        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/sessions/sess1/messages/upload"))
            .and(body_string_contains("2025-03-14T09:26:53+00:00"))
            .respond_with(ResponseTemplate::new(200).set_body_json(upload_response_json()))
            .mount(&server)
            .await;

        let msgs = session
            .upload_file(FileSource::bytes("f.txt", b"data", "text/plain"))
            .peer("alice")
            .created_at(dt)
            .send()
            .await
            .unwrap();

        assert_eq!(msgs.len(), 1);
    }

    #[tokio::test]
    async fn upload_file_with_path_reads_file_and_uploads() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("notes.txt");
        std::fs::write(&file_path, "file from disk").unwrap();

        let server = MockServer::start().await;
        let http =
            HttpClient::from_params(HttpClient::builder().base_url(server.uri()).build()).unwrap();
        let session = make_session(http, "sess1");

        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/sessions/sess1/messages/upload"))
            .and(body_string_contains("file from disk"))
            .respond_with(ResponseTemplate::new(200).set_body_json(upload_response_json()))
            .mount(&server)
            .await;

        let msgs = session
            .upload_file(FileSource::path(&file_path))
            .peer("alice")
            .send()
            .await
            .unwrap();

        assert_eq!(msgs.len(), 1);
    }

    #[tokio::test]
    async fn upload_file_path_without_filename_returns_validation_error() {
        // Regression: a path with no final file-name component (`/`, trailing
        // `..`) must fail fast on the PRODUCTION upload path before any request
        // is made, rather than silently uploading with an empty filename.
        let server = MockServer::start().await;
        let http =
            HttpClient::from_params(HttpClient::builder().base_url(server.uri()).build()).unwrap();
        let session = make_session(http, "sess1");

        let err = session
            .upload_file(FileSource::path("/"))
            .peer("alice")
            .send()
            .await
            .unwrap_err();

        assert_eq!(err.code(), "validation_error");
    }

    #[tokio::test]
    async fn upload_file_without_peer_returns_validation_error() {
        let server = MockServer::start().await;
        let http =
            HttpClient::from_params(HttpClient::builder().base_url(server.uri()).build()).unwrap();
        let session = make_session(http, "sess1");

        let err = session
            .upload_file(FileSource::bytes("f.txt", b"data", "text/plain"))
            .send()
            .await
            .unwrap_err();

        assert_eq!(err.code(), "validation_error");
    }

    #[tokio::test]
    async fn upload_file_streamed_uses_reader_stream() {
        let server = MockServer::start().await;
        let http =
            HttpClient::from_params(HttpClient::builder().base_url(server.uri()).build()).unwrap();
        let session = make_session(http, "sess1");

        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/sessions/sess1/messages/upload"))
            .and(body_string_contains("streamed payload"))
            .and(body_string_contains("peer_id"))
            .and(body_string_contains("carol"))
            .respond_with(ResponseTemplate::new(200).set_body_json(upload_response_json()))
            .mount(&server)
            .await;

        let cursor = std::io::Cursor::new(b"streamed payload".to_vec());
        let msgs = session
            .upload_file_streamed("doc.txt", cursor, "text/plain")
            .peer("carol")
            .send()
            .await
            .unwrap();

        assert_eq!(msgs.len(), 1);
    }

    fn peer_json(id: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "workspace_id": "ws1",
            "created_at": "2025-01-15T10:30:00Z",
            "metadata": {},
            "configuration": {}
        })
    }

    #[tokio::test]
    async fn peers_traverses_all_pages() {
        use wiremock::matchers::query_param;

        let server = MockServer::start().await;
        let http =
            HttpClient::from_params(HttpClient::builder().base_url(server.uri()).build()).unwrap();
        let session = make_session(http, "sess1");

        Mock::given(method("GET"))
            .and(path("/v3/workspaces/ws1/sessions/sess1/peers"))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [peer_json("alice")],
                "total": 2,
                "page": 1,
                "size": 1,
                "pages": 2
            })))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/v3/workspaces/ws1/sessions/sess1/peers"))
            .and(query_param("page", "2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [peer_json("bob")],
                "total": 2,
                "page": 2,
                "size": 1,
                "pages": 2
            })))
            .mount(&server)
            .await;

        let peers = session.peers().await.unwrap();
        assert_eq!(peers.len(), 2);
        assert_eq!(peers[0].id(), "alice");
        assert_eq!(peers[1].id(), "bob");
    }

    #[tokio::test]
    async fn refresh_uses_get_or_create_and_updates_all_cache_fields() {
        let server = MockServer::start().await;
        let http =
            HttpClient::from_params(HttpClient::builder().base_url(server.uri()).build()).unwrap();
        let session = make_session(http, "sess1");
        assert!(session.is_active());

        // No `GET /sessions/{id}` exists server-side; reads go through the
        // get-or-create `POST /sessions` collection endpoint.
        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/sessions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "sess1",
                "workspace_id": "ws1",
                "is_active": false,
                "metadata": {"topic": "fresh"},
                "configuration": {},
                "created_at": "2025-01-15T10:30:00Z"
            })))
            .mount(&server)
            .await;

        session.refresh().await.unwrap();
        assert!(!session.is_active());
        assert_eq!(session.metadata().unwrap().get("topic").unwrap(), "fresh");
    }

    // A real get-or-create `POST /sessions` re-creates a missing session (200),
    // so "deleted session" never yields 404 in practice; this just verifies the
    // SDK surfaces a server 404 as `NotFound` rather than swallowing it.
    #[tokio::test]
    async fn refresh_surfaces_server_404_as_not_found() {
        let server = MockServer::start().await;
        let http =
            HttpClient::from_params(HttpClient::builder().base_url(server.uri()).build()).unwrap();
        let session = make_session(http, "sess1");

        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/sessions"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let err = session.refresh().await.unwrap_err();
        assert_eq!(err.code(), "not_found");
    }

    #[tokio::test]
    async fn set_metadata_keeps_is_active_fresh() {
        let server = MockServer::start().await;
        let http =
            HttpClient::from_params(HttpClient::builder().base_url(server.uri()).build()).unwrap();
        let session = make_session(http, "sess1");
        assert!(session.is_active());

        // The server flips is_active in its PUT response: the single-lock cache
        // must refresh is_active too, not just metadata.
        Mock::given(method("PUT"))
            .and(path("/v3/workspaces/ws1/sessions/sess1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "sess1",
                "workspace_id": "ws1",
                "is_active": false,
                "metadata": {"updated": true},
                "configuration": {},
                "created_at": "2025-01-15T10:30:00Z"
            })))
            .mount(&server)
            .await;

        let mut meta = HashMap::new();
        meta.insert("updated".to_owned(), serde_json::json!(true));
        session.set_metadata(meta).await.unwrap();

        assert!(!session.is_active());
        assert_eq!(session.metadata().unwrap().get("updated").unwrap(), true);
    }

    #[tokio::test]
    async fn upload_invalid_content_type_returns_validation_error() {
        let server = MockServer::start().await;
        let http =
            HttpClient::from_params(HttpClient::builder().base_url(server.uri()).build()).unwrap();
        let session = make_session(http, "sess1");

        let err = session
            .upload_file(FileSource::bytes("f.txt", b"data", "text/plain\n"))
            .peer("alice")
            .send()
            .await
            .unwrap_err();

        assert_eq!(err.code(), "validation_error");
    }

    #[tokio::test]
    async fn add_peers_duplicate_ids_returns_validation_error() {
        let server = MockServer::start().await;
        let http =
            HttpClient::from_params(HttpClient::builder().base_url(server.uri()).build()).unwrap();
        let session = make_session(http, "sess1");

        let err = session.add_peers(["alice", "alice"]).await.unwrap_err();
        assert_eq!(err.code(), "validation_error");
    }
}
