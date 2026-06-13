//! Internal tokio runtime backing the sync (blocking) API.
//!
//! The runtime is created lazily on first use and intentionally lives for the
//! process lifetime — there is no shutdown path (see [`RUNTIME`]).

use std::future::Future;
use std::sync::OnceLock;
use tokio::runtime::{Handle, Runtime};

use crate::error::HonchoError;

/// Canonical message returned when the blocking API is driven from inside an
/// async runtime. Shared with other call sites that surface the same diagnostic.
pub(crate) const BLOCKING_IN_ASYNC_CTX_MSG: &str =
    "blocking API cannot be used from inside an async runtime; use the async Honcho client instead";

/// Lazily-created global multi-thread tokio runtime.
///
/// Stored as an [`std::io::Result`] so that a failure to build the runtime
/// (e.g. OS thread/fd limits exhausted on the very first SDK call) is reported
/// as a [`HonchoError::Configuration`] instead of panicking the host process.
///
/// The runtime is intentionally never shut down: it is an idempotent singleton
/// owned by the process, and tearing it down on `Drop` would break any in-flight
/// blocking call still holding a borrowed [`Handle`].
static RUNTIME: OnceLock<std::io::Result<Runtime>> = OnceLock::new();

/// Returns worker count: `min(available_parallelism, 8)`, default 4 when the
/// platform cannot report parallelism.
///
/// The 8-worker cap is currently hardcoded; a public builder knob to override
/// it is out of scope for this PR (signature change deferred to a follow-up).
fn worker_count() -> usize {
    std::thread::available_parallelism().map_or(4, |n| n.get().min(8))
}

fn get_or_create_runtime() -> &'static std::io::Result<Runtime> {
    RUNTIME.get_or_init(|| {
        // new_current_thread() is forbidden here: upload_file_streamed spawns a
        // dedicated OS thread that drives the multipart body via
        // Handle::block_on, which needs worker threads to make progress — a
        // current-thread runtime would deadlock that reader thread.
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(worker_count())
            .enable_all()
            .build()
    })
}

fn runtime_build_error(e: &std::io::Error) -> HonchoError {
    HonchoError::Configuration(format!("failed to create honcho-ai blocking runtime: {e}"))
}

/// Obtain a [`Handle`] to the global runtime, creating it on first call.
///
/// # Contract
///
/// The returned handle **must only be driven from a plain (non-async) thread**.
/// It is the seam used by
/// [`upload_file_streamed`](crate::blocking::Session::upload_file_streamed) to
/// bridge a synchronous reader onto an async multipart stream. Calling
/// `handle.block_on(...)` from inside another async runtime bypasses the
/// [`block_on`] guard and triggers tokio's own "cannot start a runtime from
/// within a runtime" panic.
///
/// # Errors
///
/// Returns [`HonchoError::Configuration`] if the global runtime could not be
/// built (e.g. resource limits).
pub(crate) fn handle() -> crate::error::Result<Handle> {
    let runtime = get_or_create_runtime()
        .as_ref()
        .map_err(runtime_build_error)?;
    Ok(runtime.handle().clone())
}

/// Run `future` to completion on the global runtime.
///
/// This is the single funnel every sync (blocking) SDK method goes through.
///
/// # Guard
///
/// Refuses to run when a tokio runtime is already bound to the current thread
/// (`Handle::try_current()` succeeds): nesting runtimes panics inside tokio.
/// The rejection is intentionally conservative — it also trips for legitimate
/// `spawn_blocking` / `block_in_place` contexts where `Runtime::block_on` would
/// in fact be legal, and it cannot detect non-tokio executors. Broadening the
/// detection is risky and out of scope for this PR; the actionable error
/// message ([`BLOCKING_IN_ASYNC_CTX_MSG`]) points users at the async client.
///
/// # Errors
///
/// Returns [`HonchoError::Configuration`] (with [`BLOCKING_IN_ASYNC_CTX_MSG`])
/// when called from inside an async runtime, or if the global runtime could not
/// be built.
pub(crate) fn block_on<F: Future>(future: F) -> crate::error::Result<F::Output> {
    if Handle::try_current().is_ok() {
        return Err(HonchoError::Configuration(
            BLOCKING_IN_ASYNC_CTX_MSG.to_owned(),
        ));
    }
    let runtime = get_or_create_runtime()
        .as_ref()
        .map_err(runtime_build_error)?;
    Ok(runtime.block_on(future))
}
