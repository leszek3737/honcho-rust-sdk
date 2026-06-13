use std::collections::HashMap;
use std::io::{self, Read};
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};

use chrono::{DateTime, Utc};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncWriteExt};

use crate::FileSource;
use crate::error::{HonchoError, Result};
use crate::session::PeerSpec;
use crate::types::message::MessageSearchOptions;
use crate::types::session::{SessionConfiguration, SessionPeerConfig};

use super::runtime::block_on;

/// Capacity (bytes) of the synchronous-reader → async-multipart duplex pipe and
/// of the read buffer used by [`Session::upload_file_streamed`]. Kept as a
/// single named constant so the two halves of the pipeline stay in sync.
const PIPE_BUF: usize = 8192;

struct ErrorAwareReader {
    inner: tokio::io::DuplexStream,
    error_slot: Arc<Mutex<Option<io::Error>>>,
}

impl AsyncRead for ErrorAwareReader {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        // AsyncRead defines EOF as no *growth* in filled(), not an empty
        // buffer: a caller may hand us a pre-filled ReadBuf, in which case
        // filled() is non-empty at genuine EOF. Capture the length before
        // delegating so the error_slot is consulted exactly once at EOF.
        let before = buf.filled().len();
        match Pin::new(&mut this.inner).poll_read(cx, buf) {
            Poll::Ready(Ok(())) => {
                if buf.filled().len() == before
                    && let Some(err) = this
                        .error_slot
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .take()
                {
                    return Poll::Ready(Err(err));
                }
                Poll::Ready(Ok(()))
            }
            other => other,
        }
    }
}

/// Synchronous wrapper around [`crate::Session`].
#[derive(Clone)]
pub struct Session {
    inner: crate::Session,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("id", &self.inner.id())
            .field("is_active", &self.inner.is_active())
            .finish()
    }
}

impl Session {
    pub(crate) fn new(inner: crate::Session) -> Self {
        Self { inner }
    }

    /// Session's unique identifier.
    #[must_use]
    pub fn id(&self) -> &str {
        self.inner.id()
    }

    /// Whether the session is active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.inner.is_active()
    }

    /// Cached metadata.
    #[must_use]
    pub fn metadata(&self) -> Option<HashMap<String, Value>> {
        self.inner.metadata()
    }

    /// Cached configuration.
    #[must_use]
    pub fn configuration(&self) -> Option<SessionConfiguration> {
        self.inner.configuration()
    }

    /// Refresh cached state from the server.
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError`] on network/HTTP failure, response parse failure,
    /// or if invoked from inside an async runtime (use the async client instead).
    pub fn refresh(&self) -> Result<()> {
        block_on(self.inner.refresh())?
    }

    /// Fetch and return metadata.
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError`] on network/HTTP failure, response parse failure,
    /// or if invoked from inside an async runtime (use the async client instead).
    pub fn get_metadata(&self) -> Result<HashMap<String, Value>> {
        block_on(self.inner.get_metadata())?
    }

    /// Set metadata on the server.
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError`] on network/HTTP failure, response parse failure,
    /// or if invoked from inside an async runtime (use the async client instead).
    pub fn set_metadata(&self, metadata: HashMap<String, Value>) -> Result<()> {
        block_on(self.inner.set_metadata(metadata))?
    }

    /// Fetch and return configuration.
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError`] on network/HTTP failure, response parse failure,
    /// or if invoked from inside an async runtime (use the async client instead).
    pub fn get_configuration(&self) -> Result<SessionConfiguration> {
        block_on(self.inner.get_configuration())?
    }

    /// Set configuration on the server.
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError`] on network/HTTP failure, response parse failure,
    /// or if invoked from inside an async runtime (use the async client instead).
    pub fn set_configuration(&self, configuration: &SessionConfiguration) -> Result<()> {
        block_on(self.inner.set_configuration(configuration))?
    }

    /// Fetch configuration as a raw JSON map.
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError`] on network/HTTP failure, response parse failure,
    /// or if invoked from inside an async runtime (use the async client instead).
    pub fn get_configuration_raw(&self) -> Result<HashMap<String, Value>> {
        block_on(self.inner.get_configuration_raw())?
    }

    /// Set configuration from a raw JSON map.
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError`] on network/HTTP failure, response parse failure,
    /// or if invoked from inside an async runtime (use the async client instead).
    pub fn set_configuration_raw(&self, configuration: HashMap<String, Value>) -> Result<()> {
        block_on(self.inner.set_configuration_raw(configuration))?
    }

    /// Add a single peer to this session.
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError`] on network/HTTP failure, response parse failure,
    /// or if invoked from inside an async runtime (use the async client instead).
    pub fn add_peer(&self, id: impl Into<String>) -> Result<()> {
        block_on(self.inner.add_peer(id))?
    }

    /// Add multiple peers.
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError`] on network/HTTP failure, response parse failure,
    /// or if invoked from inside an async runtime (use the async client instead).
    pub fn add_peers(&self, specs: impl IntoIterator<Item = impl Into<PeerSpec>>) -> Result<()> {
        block_on(self.inner.add_peers(specs))?
    }

    /// Set the complete peer list (replaces existing).
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError`] on network/HTTP failure, response parse failure,
    /// or if invoked from inside an async runtime (use the async client instead).
    pub fn set_peers(&self, specs: impl IntoIterator<Item = impl Into<PeerSpec>>) -> Result<()> {
        block_on(self.inner.set_peers(specs))?
    }

    /// Remove peers from this session.
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError`] on network/HTTP failure, response parse failure,
    /// or if invoked from inside an async runtime (use the async client instead).
    pub fn remove_peers(&self, ids: impl IntoIterator<Item = impl Into<String>>) -> Result<()> {
        block_on(self.inner.remove_peers(ids))?
    }

    /// List peers in this session.
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError`] on network/HTTP failure, response parse failure,
    /// or if invoked from inside an async runtime (use the async client instead).
    pub fn peers(&self) -> Result<Vec<super::Peer>> {
        block_on(self.inner.peers())?.map(|peers| peers.into_iter().map(super::Peer::new).collect())
    }

    /// Get per-peer configuration.
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError`] on network/HTTP failure, response parse failure,
    /// or if invoked from inside an async runtime (use the async client instead).
    pub fn get_peer_configuration(&self, peer_id: &str) -> Result<SessionPeerConfig> {
        block_on(self.inner.get_peer_configuration(peer_id))?
    }

    /// Set per-peer configuration.
    ///
    /// The peer must already be present in the session. This method does not
    /// create or add peers; use [`Session::add_peer`] or [`Session::add_peers`]
    /// first. If the peer is absent, the server may return 404/`NotFound`.
    /// # Errors
    ///
    /// Returns [`HonchoError`] on network/HTTP failure, response parse failure,
    /// or if invoked from inside an async runtime (use the async client instead).
    pub fn set_peer_configuration(&self, peer_id: &str, config: &SessionPeerConfig) -> Result<()> {
        block_on(self.inner.set_peer_configuration(peer_id, config))?
    }

    /// Add messages to this session.
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError`] on network/HTTP failure, response parse failure,
    /// or if invoked from inside an async runtime (use the async client instead).
    pub fn add_messages(
        &self,
        messages: Vec<crate::types::message::MessageCreate>,
    ) -> Result<Vec<crate::Message>> {
        block_on(self.inner.add_messages(messages))?
    }

    /// List messages, collecting across pages.
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError`] on network/HTTP failure, response parse failure,
    /// or if invoked from inside an async runtime (use the async client instead).
    pub fn messages(&self) -> Result<Vec<crate::Message>> {
        block_on(async {
            let page = self.inner.messages().await?;
            super::iter::collect_all_pages(page).await
        })?
    }

    /// Delete this session.
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError`] on network/HTTP failure, response parse failure,
    /// or if invoked from inside an async runtime (use the async client instead).
    pub fn delete(&self) -> Result<()> {
        block_on(self.inner.delete())?
    }

    /// Create a server-side copy of this session.
    ///
    /// Despite the name, this does not [`Clone`] the [`Session`] handle
    /// in-process: the server creates a new session with a fresh session ID.
    /// Use [`Clone::clone`] on the handle if you only need a second reference to
    /// the same session.
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError`] on network/HTTP failure, response parse failure,
    /// or if invoked from inside an async runtime (use the async client instead).
    pub fn clone_session(&self) -> Result<Session> {
        block_on(self.inner.clone_session())?.map(Session::new)
    }

    /// Create a server-side copy of this session up to a message.
    ///
    /// Like [`clone_session`](Session::clone_session), this is a server-side
    /// copy with a new session ID, not an in-process [`Clone`].
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError`] on network/HTTP failure, response parse failure,
    /// or if invoked from inside an async runtime (use the async client instead).
    pub fn clone_session_with_message(&self, message_id: &str) -> Result<Session> {
        block_on(self.inner.clone_session_with_message(message_id))?.map(Session::new)
    }

    /// Get a single message by ID.
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError`] on network/HTTP failure, response parse failure,
    /// or if invoked from inside an async runtime (use the async client instead).
    pub fn get_message(&self, id: &str) -> Result<crate::Message> {
        block_on(self.inner.get_message(id))?
    }

    /// Update a message's metadata.
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError`] on network/HTTP failure, response parse failure,
    /// or if invoked from inside an async runtime (use the async client instead).
    pub fn update_message(
        &self,
        id: &str,
        metadata: HashMap<String, Value>,
    ) -> Result<crate::Message> {
        block_on(self.inner.update_message(id, metadata))?
    }

    /// Get session context.
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError`] on network/HTTP failure, response parse failure,
    /// or if invoked from inside an async runtime (use the async client instead).
    pub fn context(&self) -> Result<crate::types::session::SessionContext> {
        block_on(self.inner.context())?
    }

    /// Get session context with custom parameters.
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError`] on network/HTTP failure, response parse failure,
    /// or if invoked from inside an async runtime (use the async client instead).
    pub fn context_with_options(
        &self,
        options: &crate::types::session::SessionContextOptions,
    ) -> Result<crate::types::session::SessionContext> {
        block_on(self.inner.context_with_options(options))?
    }

    /// Get a context builder for fine-grained control over session context parameters.
    pub fn context_builder(&self) -> BlockingSessionContextBuilder {
        BlockingSessionContextBuilder {
            inner: self.inner.context_builder(),
        }
    }

    /// Get available summaries.
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError`] on network/HTTP failure, response parse failure,
    /// or if invoked from inside an async runtime (use the async client instead).
    pub fn summaries(&self) -> Result<crate::types::session::SessionSummaries> {
        block_on(self.inner.summaries())?
    }

    /// Search messages within this session.
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError`] on network/HTTP failure, response parse failure,
    /// or if invoked from inside an async runtime (use the async client instead).
    pub fn search(&self, query: &str) -> Result<Vec<crate::Message>> {
        block_on(self.inner.search(query))?
    }

    /// Search messages within this session with custom options.
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError`] on network/HTTP failure, response parse failure,
    /// or if invoked from inside an async runtime (use the async client instead).
    pub fn search_with_options(
        &self,
        options: &MessageSearchOptions,
    ) -> Result<Vec<crate::Message>> {
        block_on(self.inner.search_with_options(options))?
    }

    /// Get a peer's representation scoped to this session.
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError`] on network/HTTP failure, response parse failure,
    /// or if invoked from inside an async runtime (use the async client instead).
    pub fn representation(&self, peer_id: &str) -> Result<String> {
        block_on(self.inner.representation(peer_id))?
    }

    /// Get a representation builder for a peer in this session.
    ///
    /// The returned [`BlockingSessionRepresentationBuilder`] is configured via
    /// its builder methods and executed with `.send()`.
    pub fn representation_builder(
        &self,
        peer_id: impl Into<String>,
    ) -> BlockingSessionRepresentationBuilder {
        BlockingSessionRepresentationBuilder {
            inner: self.inner.representation_builder(peer_id.into()),
        }
    }

    /// Get processing queue status for this session.
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError`] on network/HTTP failure, response parse failure,
    /// or if invoked from inside an async runtime (use the async client instead).
    pub fn queue_status(
        &self,
        observer_id: Option<&str>,
        sender_id: Option<&str>,
    ) -> Result<crate::types::dream::QueueStatus> {
        block_on(self.inner.queue_status(observer_id, sender_id))?
    }

    /// Begin a file upload to this session.
    ///
    /// The API currently accepts `text/plain`, `application/pdf`, and
    /// `application/json`; other MIME types may be rejected by the server.
    ///
    /// Returns a [`BlockingUploadFileBuilder`]. You **must** call `.peer(id)`
    /// and then `.send()` to complete the upload.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(session: &honcho_ai::blocking::Session) -> honcho_ai::error::Result<()> {
    /// let msgs = session
    ///     .upload_file(honcho_ai::FileSource::bytes("doc.pdf", b"data", "application/pdf"))
    ///     .peer("alice")
    ///     .send()?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn upload_file(&self, source: impl Into<FileSource>) -> BlockingUploadFileBuilder<'_> {
        BlockingUploadFileBuilder {
            inner: self.inner.upload_file(source),
            reader_handle: None,
        }
    }

    /// Begin a file upload from a synchronous reader.
    ///
    /// The API currently accepts `text/plain`, `application/pdf`, and
    /// `application/json`; other MIME types may be rejected by the server.
    ///
    /// The reader is consumed in a background OS thread and piped to the async
    /// multipart stream **without buffering the entire file in memory**. A
    /// plain OS thread (not `tokio::task::spawn_blocking`) is used so the reader
    /// runs even without an ambient tokio runtime, and the tokio blocking pool
    /// is left free; the body is driven via the global runtime's [`Handle`].
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError::Io`](crate::error::HonchoError::Io) if reading
    /// from `reader` fails during upload, [`HonchoError::Configuration`] if the
    /// global runtime could not be built (surfaced via the stream at `send()`
    /// time), or another [`HonchoError`] variant if the server rejects the
    /// upload.
    ///
    /// [`Handle`]: tokio::runtime::Handle
    #[must_use]
    pub fn upload_file_streamed(
        &self,
        filename: impl Into<String>,
        reader: impl Read + Send + 'static,
        content_type: impl Into<String>,
    ) -> BlockingUploadFileBuilder<'_> {
        let (mut tx, rx) = tokio::io::duplex(PIPE_BUF);
        let filename_owned = filename.into();
        let content_type_owned = content_type.into();
        let error_slot: Arc<Mutex<Option<io::Error>>> = Arc::new(Mutex::new(None));
        let slot_clone = error_slot.clone();
        let slot_for_err = error_slot.clone();
        let handle = super::runtime::handle();

        // If the runtime could not be built, there is no way to drive the
        // reader; stash the error in the shared slot so ErrorAwareReader
        // surfaces it at EOF and skip spawning the reader thread. send() will
        // therefore fail with the same configuration error.
        let reader_handle: Option<std::thread::JoinHandle<()>> = match handle {
            Ok(handle) => Some(std::thread::spawn(move || {
                let mut reader = reader;
                handle.block_on(async {
                    let mut buf = [0u8; PIPE_BUF];
                    loop {
                        match reader.read(&mut buf) {
                            Ok(0) => return,
                            Ok(n) => {
                                if tx.write_all(&buf[..n]).await.is_err() {
                                    return;
                                }
                            }
                            Err(e) => {
                                *slot_clone.lock().unwrap_or_else(PoisonError::into_inner) =
                                    Some(e);
                                return;
                            }
                        }
                    }
                });
            })),
            Err(e) => {
                *slot_for_err.lock().unwrap_or_else(PoisonError::into_inner) =
                    Some(std::io::Error::other(e.to_string()));
                None
            }
        };

        let wrapped_rx = ErrorAwareReader {
            inner: rx,
            error_slot,
        };

        BlockingUploadFileBuilder {
            inner: self.inner.upload_file(FileSource::stream(
                filename_owned,
                wrapped_rx,
                content_type_owned,
            )),
            reader_handle,
        }
    }
}

/// Blocking wrapper around [`crate::UploadFileBuilder`].
pub struct BlockingUploadFileBuilder<'a> {
    inner: crate::UploadFileBuilder<'a>,
    reader_handle: Option<std::thread::JoinHandle<()>>,
}

impl std::fmt::Debug for BlockingUploadFileBuilder<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlockingUploadFileBuilder")
            .finish_non_exhaustive()
    }
}

impl BlockingUploadFileBuilder<'_> {
    /// Set the peer that owns the uploaded file (required).
    #[must_use]
    pub fn peer(mut self, id: impl Into<String>) -> Self {
        self.inner = self.inner.peer(id);
        self
    }

    /// Attach arbitrary JSON metadata to the created message(s).
    #[must_use]
    pub fn metadata(mut self, value: Value) -> Self {
        self.inner = self.inner.metadata(value);
        self
    }

    /// Attach configuration to the created message(s).
    #[must_use]
    pub fn configuration(mut self, value: Value) -> Self {
        self.inner = self.inner.configuration(value);
        self
    }

    /// Override the creation timestamp (RFC 3339).
    #[must_use]
    pub fn created_at(mut self, dt: DateTime<Utc>) -> Self {
        self.inner = self.inner.created_at(dt);
        self
    }

    /// Send the upload request and return the created messages.
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError::Validation`](crate::error::HonchoError::Validation)
    /// if no peer was set via `.peer()`, [`HonchoError::Io`] if reading from the
    /// synchronous `reader` failed or its background thread panicked, or another
    /// [`HonchoError`] variant if the server rejected the upload.
    pub fn send(self) -> Result<Vec<crate::Message>> {
        // block_on returns Result<Result<Vec<_>, HonchoError>, HonchoError>:
        // the outer error is the runtime guard / build failure, the inner is
        // the server response. Flatten both so is_ok() reflects server success
        // while still letting the join below run on every exit path.
        let send_result = block_on(self.inner.send());
        let send_result: Result<Vec<crate::Message>> = send_result.and_then(std::convert::identity);

        // Always join the reader thread when present so it never outlives this
        // call — even if send() already failed. This prevents the background
        // thread from continuing to read the user's reader after we return.
        if let Some(handle) = self.reader_handle
            && let Err(panic_payload) = handle.join()
            && send_result.is_ok()
        {
            // std::thread::JoinHandle::join returns Err(Box<dyn Any + Send>)
            // on a panic; downcast to preserve the panic message for diagnosis.
            let msg = panic_payload
                .downcast_ref::<&'static str>()
                .copied()
                .or_else(|| panic_payload.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("reader thread panicked");
            return Err(HonchoError::Io(std::io::Error::other(msg)));
        }

        send_result
    }
}

/// Blocking builder for session representation queries.
///
/// Wraps the async `SessionRepresentationBuilder`.
#[must_use]
pub struct BlockingSessionRepresentationBuilder {
    inner: super::super::session::SessionRepresentationBuilder,
}

impl std::fmt::Debug for BlockingSessionRepresentationBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlockingSessionRepresentationBuilder")
            .finish_non_exhaustive()
    }
}

impl BlockingSessionRepresentationBuilder {
    /// Set the target peer.
    pub fn target(mut self, target: impl Into<String>) -> Self {
        self.inner = self.inner.target(target);
        self
    }

    /// Set a semantic search query.
    pub fn search_query(mut self, query: impl Into<String>) -> Self {
        self.inner = self.inner.search_query(query);
        self
    }

    /// Set the number of top search results.
    pub fn search_top_k(mut self, k: u32) -> Self {
        self.inner = self.inner.search_top_k(k);
        self
    }

    /// Set the maximum search distance.
    pub fn search_max_distance(mut self, d: f64) -> Self {
        self.inner = self.inner.search_max_distance(d);
        self
    }

    /// Include the most frequent conclusions.
    pub fn include_most_frequent(mut self, v: bool) -> Self {
        self.inner = self.inner.include_most_frequent(v);
        self
    }

    /// Set the maximum number of conclusions.
    pub fn max_conclusions(mut self, m: u32) -> Self {
        self.inner = self.inner.max_conclusions(m);
        self
    }

    /// Execute the request and return the representation.
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError`] on network/HTTP failure, response parse failure,
    /// or if invoked from inside an async runtime (use the async client instead).
    pub fn send(self) -> Result<String> {
        block_on(self.inner.send())?
    }
}

/// Blocking builder for session context queries.
///
/// Wraps the async `SessionContextBuilder`.
#[must_use]
pub struct BlockingSessionContextBuilder {
    inner: crate::session::SessionContextBuilder,
}

impl std::fmt::Debug for BlockingSessionContextBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlockingSessionContextBuilder")
            .finish_non_exhaustive()
    }
}

impl BlockingSessionContextBuilder {
    /// Whether to include summaries (default: `true`).
    pub fn summary(mut self, val: bool) -> Self {
        self.inner = self.inner.summary(val);
        self
    }

    /// Limit context to this session only (default: `false`).
    pub fn limit_to_session(mut self, val: bool) -> Self {
        self.inner = self.inner.limit_to_session(val);
        self
    }

    /// Maximum number of tokens for the context.
    pub fn tokens(mut self, val: u32) -> Self {
        self.inner = self.inner.tokens(val);
        self
    }

    /// Target peer for perspective-based context.
    pub fn peer_target(mut self, val: impl Into<String>) -> Self {
        self.inner = self.inner.peer_target(val);
        self
    }

    /// Perspective peer for viewing context.
    pub fn peer_perspective(mut self, val: impl Into<String>) -> Self {
        self.inner = self.inner.peer_perspective(val);
        self
    }

    /// Semantic search query to filter relevant conclusions.
    pub fn search_query(mut self, val: impl Into<String>) -> Self {
        self.inner = self.inner.search_query(val);
        self
    }

    /// Number of semantic-search-retrieved conclusions (1–100).
    pub fn search_top_k(mut self, k: u32) -> Self {
        self.inner = self.inner.search_top_k(k);
        self
    }

    /// Maximum distance for semantically relevant conclusions (0.0–1.0).
    pub fn search_max_distance(mut self, d: f64) -> Self {
        self.inner = self.inner.search_max_distance(d);
        self
    }

    /// Include the most frequent conclusions.
    pub fn include_most_frequent(mut self, v: bool) -> Self {
        self.inner = self.inner.include_most_frequent(v);
        self
    }

    /// Maximum number of conclusions to include (1–100).
    pub fn max_conclusions(mut self, m: u32) -> Self {
        self.inner = self.inner.max_conclusions(m);
        self
    }

    /// Execute the request and return the session context.
    ///
    /// # Errors
    ///
    /// Returns [`HonchoError`] on network/HTTP failure, response parse failure,
    /// or if invoked from inside an async runtime (use the async client instead).
    pub fn send(self) -> Result<crate::types::session::SessionContext> {
        block_on(self.inner.send())?
    }
}
