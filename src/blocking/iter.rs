//! Blocking iterator adapter for async streams.
//!
//! [`BlockingIter`] drives an async [`Stream`] on the blocking facade's
//! internal tokio runtime, exposing it as a synchronous [`Iterator`]. The
//! adapter never panics: if a poll is attempted from inside an async runtime
//! (where the internal runtime cannot be entered),
//! [`BlockingIter::try_next`] returns a
//! [`Configuration`](crate::error::HonchoError::Configuration) error instead.
//! `Result`-yielding callers (such as
//! [`ChatStreamIterator`](super::peer::ChatStreamIterator)) inject that error
//! as `Some(Err(..))` so iteration continues to honor the [`Iterator`]
//! contract without panicking.

use std::future::poll_fn;
use std::pin::Pin;
use std::task::Context;

use futures_util::Stream;

use super::runtime::block_on;

/// Maximum number of pages [`collect_all_pages`] will fetch before raising a
/// safety-cap error.
const MAX_COLLECT_PAGES: u32 = 1_000;

/// Preallocation ceiling for the collected output vector. The server-reported
/// total may be huge or attacker-controlled, so we cap the initial allocation
/// and let the `Vec` grow organically beyond it.
const COLLECT_PREALLOC_CAP: usize = 10_000;

/// Synchronous iterator over an async [`Stream`].
///
/// Drives the wrapped stream on the blocking facade's internal tokio runtime.
/// Never panics: context-rejection from [`block_on`] is surfaced via
/// [`try_next`](Self::try_next) as a `Configuration` error rather than via
/// `expect`/`panic`.
///
/// The generic [`Iterator`] impl is intentionally bounds-light so this type
/// can wrap both `Result`- and non-`Result`-yielding streams. `Result`-yielding
/// callers should prefer [`try_next`](Self::try_next) and inject the
/// context-rejection as `Some(Err(..))` themselves; the generic
/// [`Iterator::next`] silently maps a context-rejection to `None` because the
/// `Item` type gives it no other channel for the error.
pub(crate) struct BlockingIter<S> {
    stream: Pin<Box<S>>,
}

impl<S> BlockingIter<S> {
    /// Wrap a stream for synchronous iteration on the blocking runtime.
    pub(crate) fn new(stream: S) -> Self {
        Self {
            stream: Box::pin(stream),
        }
    }

    /// Borrow the underlying stream (e.g. to call `final_response`).
    pub(crate) fn stream(&self) -> &S {
        self.stream.as_ref().get_ref()
    }
}

impl<S: Stream> BlockingIter<S> {
    /// Advance the wrapped stream on the blocking runtime.
    ///
    /// Returns `Ok(Some(item))` when the stream yields, `Ok(None)` at
    /// end-of-stream, and `Err(_)` if the call is made from inside an active
    /// async runtime (where the blocking facade cannot safely enter its own
    /// runtime). Never panics.
    pub(crate) fn try_next(&mut self) -> crate::error::Result<Option<S::Item>> {
        let stream = &mut self.stream;
        block_on(poll_fn(move |cx: &mut Context<'_>| {
            stream.as_mut().poll_next(cx)
        }))
    }
}

impl<S: Stream> Iterator for BlockingIter<S> {
    type Item = S::Item;

    fn next(&mut self) -> Option<Self::Item> {
        // Context-rejection cannot be surfaced generically: `Item` may not be
        // a `Result`. Callers that need the error path use `try_next` and
        // inject `Some(Err(..))` themselves. Here a context-rejection is
        // silently treated as end-of-stream — safe (the stream is undrivable
        // from this context) but lossy, hence the `try_next` recommendation.
        self.try_next().unwrap_or_default()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        // SSE-driven streams do not advertise a length, so we cannot give
        // `collect()` a useful lower bound. Stated explicitly for clarity.
        (0, None)
    }
}

/// Collect every item from a paginated response into a single `Vec`, fetching
/// subsequent pages lazily.
///
/// Enforces a [`MAX_COLLECT_PAGES`] safety cap to prevent runaway pagination
/// loops. The cap is checked **before** each network fetch: a server reporting
/// more pages than the cap permits is never contacted for the page that would
/// exceed it, and no page beyond the cap is fetched only to be discarded.
// TODO(PR6): relocate to `Page::collect_all()` once the public-path change
// is acceptable.
pub(crate) async fn collect_all_pages<TRaw, TOut>(
    first_page: crate::types::pagination::Page<TRaw, TOut>,
) -> crate::error::Result<Vec<TOut>>
where
    TRaw: Clone + 'static,
    TOut: 'static,
{
    let cap = usize::try_from(first_page.total()).unwrap_or(usize::MAX);
    let mut all = Vec::with_capacity(cap.min(COLLECT_PREALLOC_CAP));
    let mut first_items = first_page.items();
    all.append(&mut first_items);

    let mut current = first_page;
    let mut pages: u32 = 1;
    loop {
        // Check the cap BEFORE the network fetch: page (pages + 1) would push
        // us past MAX_COLLECT_PAGES, so we error out without ever contacting
        // the server for it. If the server reports no further pages, we exit
        // cleanly instead.
        if pages >= MAX_COLLECT_PAGES && current.has_next() {
            // TODO(PR2): revisit variant once the dedicated safety-cap error
            // lands; `Configuration` is the closest current fit because the
            // caller's pagination setup is what made the cap reachable.
            return Err(crate::error::HonchoError::Configuration(format!(
                "pagination exceeded {MAX_COLLECT_PAGES} pages; refusing to fetch more to enforce the loop safety cap"
            )));
        }
        let Some(next) = current.next_page().await? else {
            break;
        };
        pages += 1;
        let mut next_items = next.items();
        all.append(&mut next_items);
        current = next;
    }
    Ok(all)
}

/// Drive a page-producing future on the blocking runtime and collect every page
/// it seeds into a single `Vec`.
///
/// Centralises the `block_on(async { fetch; collect_all_pages })` shape shared
/// by every paginated blocking list method ([`Honcho::peers`], [`Session::messages`],
/// [`Peer::sessions`], …). `block_on` wraps the future's output in an outer
/// `Result` so the async-runtime guard can surface its `Configuration` error;
/// the trailing `?` unwraps that outer layer, leaving the inner
/// [`collect_all_pages`] result as the return value.
///
/// [`Honcho::peers`]: super::Honcho::peers
/// [`Session::messages`]: super::Session::messages
/// [`Peer::sessions`]: super::Peer::sessions
pub(crate) fn collect_pages<TRaw, TOut>(
    first_page_fut: impl std::future::Future<
        Output = crate::error::Result<crate::types::pagination::Page<TRaw, TOut>>,
    >,
) -> crate::error::Result<Vec<TOut>>
where
    TRaw: Clone + 'static,
    TOut: 'static,
{
    block_on(async {
        let page = first_page_fut.await?;
        collect_all_pages(page).await
    })?
}
