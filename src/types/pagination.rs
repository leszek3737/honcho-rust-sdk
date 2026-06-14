//! Generic pagination types.

// `Page<TRaw, TRaw>` deliberately repeats the same type for both parameters in
// the default `TOut = TRaw` impls below. Clippy's `mismatching_type_param_order`
// flags this even though the ordering is correct and intentional, so we allow it
// module-wide rather than annotating each individual impl.
#![allow(clippy::mismatching_type_param_order)]

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use futures_util::Stream;
use serde::de::DeserializeOwned;

use crate::error::{HonchoError, Result};
use crate::http::client::HttpClient;

type PageFetcher<TRaw> = Arc<
    dyn Fn(u64) -> Pin<Box<dyn Future<Output = Result<PageResponse<TRaw>>> + Send>> + Send + Sync,
>;

/// Maximum page size accepted by [`validate_pagination`] (inclusive).
const MAX_PAGE_SIZE: u64 = 100;

/// Query-parameter value requesting reverse ordering.
const REVERSE_TRUE: &str = "true";

/// Serde-friendly raw page response from the API.
///
/// Deserializes directly from paginated JSON responses. Convert to
/// [`Page`] for lazy-fetch and transform support via
/// [`Page::from_page_response`]. Use [`PageResponse::with_fetcher`] to
/// attach a fetcher in one step without cloning the items vector.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PageResponse<T> {
    /// The items in this page.
    pub items: Vec<T>,
    /// Total number of items across all pages.
    pub total: u64,
    /// Current page number (1-based).
    pub page: u64,
    /// Number of items per page.
    pub size: u64,
    /// Total number of pages.
    pub pages: u64,
}

impl<T> PageResponse<T> {
    /// Create a new `PageResponse`.
    #[must_use]
    pub fn new(items: Vec<T>, total: u64, page: u64, size: u64, pages: u64) -> Self {
        Self {
            items,
            total,
            page,
            size,
            pages,
        }
    }
}

impl<T> Default for PageResponse<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            total: 0,
            page: 1,
            size: 0,
            pages: 0,
        }
    }
}

impl<T: 'static> PageResponse<T> {
    /// Convert this response into a [`Page`] with an attached fetcher,
    /// without cloning the items vector.
    ///
    /// This is more efficient than `Page::from_page_response(resp).with_fetcher(f)`
    /// which clones the items during the `with_fetcher` step.
    #[must_use]
    pub fn with_fetcher<F, Fut>(self, fetcher: F) -> Page<T, T>
    where
        F: Fn(u64) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<PageResponse<T>>> + Send + 'static,
    {
        Page {
            inner: Arc::new(PageInner {
                items: self.items,
                total: self.total,
                page: self.page,
                size: self.size,
                pages: self.pages,
                next_fetcher: Some(Arc::new(move |pn| Box::pin(fetcher(pn)))),
                transform: Arc::new(std::convert::identity),
            }),
        }
    }
}

/// A page of results with lazy next-page fetching and item transform support.
///
/// `Page<TRaw, TOut>` holds raw items of type `TRaw` and lazily transforms
/// them to `TOut` via a configurable closure. The default `TOut = TRaw` uses
/// the identity transform.
///
/// Construct with [`Page::from_page_response`] or [`Page::new`], then
/// optionally chain [`Page::with_fetcher`] to enable [`Page::next_page`].
///
/// `Page` is cheaply [`Clone`] (Arc bump) and implements [`serde::Serialize`]
/// + [`serde::Deserialize`] when `TOut = TRaw`.
pub struct Page<TRaw, TOut = TRaw> {
    inner: Arc<PageInner<TRaw, TOut>>,
}

struct PageInner<TRaw, TOut> {
    items: Vec<TRaw>,
    total: u64,
    page: u64,
    size: u64,
    pages: u64,
    next_fetcher: Option<PageFetcher<TRaw>>,
    transform: Arc<dyn Fn(TRaw) -> TOut + Send + Sync>,
}

impl<TRaw: 'static, TOut: 'static> Page<TRaw, TOut> {
    /// Returns a reference to the raw (untransformed) items.
    #[must_use]
    pub fn raw_items(&self) -> &[TRaw] {
        &self.inner.items
    }

    /// Returns transformed items as an owned `Vec<TOut>`.
    ///
    /// Each raw item is cloned and passed through the transform closure.
    /// Use [`raw_items`](Self::raw_items) to avoid the clone when no
    /// transform is needed.
    #[must_use]
    pub fn items(&self) -> Vec<TOut>
    where
        TRaw: Clone,
    {
        self.inner
            .items
            .iter()
            .cloned()
            .map(|v| (self.inner.transform)(v))
            .collect()
    }

    /// Total number of items across all pages.
    #[must_use]
    pub fn total(&self) -> u64 {
        self.inner.total
    }

    /// Current page number (1-based).
    #[must_use]
    pub fn page(&self) -> u64 {
        self.inner.page
    }

    /// Number of items per page.
    #[must_use]
    pub fn size(&self) -> u64 {
        self.inner.size
    }

    /// Total number of pages.
    #[must_use]
    pub fn pages(&self) -> u64 {
        self.inner.pages
    }

    /// Whether there are more pages after this one.
    #[must_use]
    pub fn has_next(&self) -> bool {
        self.inner.page < self.inner.pages
    }

    /// Fetch the next page, if a fetcher is configured and more pages exist.
    ///
    /// Returns `Ok(Some(..))` on success, `Ok(None)` when no fetcher is set
    /// or no more pages remain, and `Err(..)` when the fetch fails.
    pub async fn next_page(&self) -> Result<Option<Self>> {
        if !self.has_next() {
            return Ok(None);
        }
        let fetcher = match self.inner.next_fetcher.as_ref() {
            Some(f) => Arc::clone(f),
            None => return Ok(None),
        };
        // Treat page-number overflow as "no more pages" rather than panicking.
        let Some(next_num) = self.inner.page.checked_add(1) else {
            return Ok(None);
        };
        let transform = Arc::clone(&self.inner.transform);
        let next_fetcher = self.inner.next_fetcher.clone();

        let resp = fetcher(next_num).await?;
        Ok(Some(Self {
            inner: Arc::new(PageInner {
                items: resp.items,
                total: resp.total,
                page: resp.page,
                size: resp.size,
                pages: resp.pages,
                next_fetcher,
                transform,
            }),
        }))
    }

    /// Attach a next-page fetcher, consuming `self` and returning a new `Page`.
    ///
    /// The fetcher receives a page number and returns a future that resolves to
    /// a [`PageResponse<TRaw>`]. The resulting page (and every page fetched
    /// through it) carries the same fetcher.
    #[must_use]
    pub fn with_fetcher<F, Fut>(self, fetcher: F) -> Self
    where
        F: Fn(u64) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<PageResponse<TRaw>>> + Send + 'static,
        TRaw: Clone,
    {
        let next_fetcher: PageFetcher<TRaw> = Arc::new(move |pn| Box::pin(fetcher(pn)));
        // Move the items/transform out of the inner when we hold the only
        // reference (the common case); otherwise fall back to cloning.
        let inner = match Arc::try_unwrap(self.inner) {
            Ok(inner) => PageInner {
                items: inner.items,
                total: inner.total,
                page: inner.page,
                size: inner.size,
                pages: inner.pages,
                next_fetcher: Some(next_fetcher),
                transform: inner.transform,
            },
            Err(arc) => PageInner {
                items: arc.items.clone(),
                total: arc.total,
                page: arc.page,
                size: arc.size,
                pages: arc.pages,
                next_fetcher: Some(next_fetcher),
                transform: Arc::clone(&arc.transform),
            },
        };
        Self {
            inner: Arc::new(inner),
        }
    }

    /// Apply a secondary transform, producing a `Page<TRaw, TNewOut>`.
    ///
    /// The new transform composes `f` after the existing one.
    ///
    /// # Warning
    ///
    /// The transform only affects the *output* of [`items`](Self::items) and
    /// [`into_stream`](Self::into_stream). The [`PartialEq`] and
    /// [`serde::Serialize`] impls (available when `TOut = TRaw`) compare and
    /// emit the **raw** items, ignoring the transform. Two pages that compare
    /// equal can therefore still yield different `items()` after a non-identity
    /// `map`.
    pub fn map<TNewOut>(
        self,
        f: impl Fn(TOut) -> TNewOut + Send + Sync + 'static,
    ) -> Page<TRaw, TNewOut>
    where
        TRaw: Clone,
    {
        // Move items/fetcher out of the inner when we hold the only reference
        // (the common case); otherwise fall back to cloning.
        let inner = match Arc::try_unwrap(self.inner) {
            Ok(inner) => {
                let prev = inner.transform;
                PageInner {
                    items: inner.items,
                    total: inner.total,
                    page: inner.page,
                    size: inner.size,
                    pages: inner.pages,
                    next_fetcher: inner.next_fetcher,
                    transform: Arc::new(move |raw| f(prev(raw))),
                }
            }
            Err(arc) => {
                let prev = Arc::clone(&arc.transform);
                PageInner {
                    items: arc.items.clone(),
                    total: arc.total,
                    page: arc.page,
                    size: arc.size,
                    pages: arc.pages,
                    next_fetcher: arc.next_fetcher.clone(),
                    transform: Arc::new(move |raw| f(prev(raw))),
                }
            }
        };
        Page {
            inner: Arc::new(inner),
        }
    }

    /// Convert this page into a stream that auto-fetches subsequent pages.
    ///
    /// Yields transformed items from the current page, then lazily fetches
    /// and yields items from each subsequent page until all pages are exhausted.
    ///
    /// If no fetcher is attached, only the current page's items are yielded.
    pub fn into_stream(self) -> impl Stream<Item = Result<TOut>> + Send + 'static
    where
        TRaw: Clone + Send + 'static,
        TOut: Send + 'static,
    {
        let has_next = self.has_next();
        // Page-number overflow means there cannot be a next page.
        let next_page_num = self.inner.page.checked_add(1);
        // Move items/fetcher/transform out of the inner when we hold the only
        // reference (the common case); otherwise fall back to cloning.
        let (items, fetcher, transform) = match Arc::try_unwrap(self.inner) {
            Ok(inner) => (inner.items, inner.next_fetcher, inner.transform),
            Err(arc) => (
                arc.items.clone(),
                arc.next_fetcher.clone(),
                Arc::clone(&arc.transform),
            ),
        };

        async_stream::try_stream! {
            for item in items {
                yield transform(item);
            }

            if let Some(fetcher) = fetcher
                && has_next
                && let Some(start) = next_page_num
            {
                let mut current_page = start;
                loop {
                    let resp = (fetcher)(current_page).await?;
                    let is_last = resp.page >= resp.pages;
                    for item in resp.items {
                        yield transform(item);
                    }
                    if is_last {
                        break;
                    }
                    // Overflow ⇒ treat as the last page instead of panicking.
                    let Some(next) = resp.page.checked_add(1) else {
                        break;
                    };
                    if next <= current_page {
                        break;
                    }
                    current_page = next;
                }
            }
        }
    }
}

impl<TRaw: 'static> Page<TRaw, TRaw> {
    /// Create a new `Page` from raw data with no fetcher (identity transform).
    #[must_use]
    pub fn new(items: Vec<TRaw>, total: u64, page: u64, size: u64, pages: u64) -> Self {
        Self {
            inner: Arc::new(PageInner {
                items,
                total,
                page,
                size,
                pages,
                next_fetcher: None,
                transform: Arc::new(std::convert::identity),
            }),
        }
    }

    /// Create a `Page` from a deserialized [`PageResponse`].
    #[must_use]
    pub fn from_page_response(resp: PageResponse<TRaw>) -> Self {
        Self::new(resp.items, resp.total, resp.page, resp.size, resp.pages)
    }

    /// Returns a slice of the raw items without cloning.
    ///
    /// This is the identity-transform equivalent of [`items`](Page::items)
    /// that avoids allocating a new `Vec`.
    #[must_use]
    pub fn items_ref(&self) -> &[TRaw] {
        &self.inner.items
    }

    /// Consume the page and return the items, avoiding cloning when possible.
    ///
    /// If this is the only reference to the inner data, the items are moved
    /// out without cloning. Otherwise, falls back to cloning each item.
    #[must_use]
    pub fn into_items(self) -> Vec<TRaw>
    where
        TRaw: Clone,
    {
        match Arc::try_unwrap(self.inner) {
            Ok(inner) => inner.items,
            Err(arc) => arc.items.clone(),
        }
    }
}

impl<TRaw: 'static> Default for Page<TRaw, TRaw> {
    fn default() -> Self {
        Self {
            inner: Arc::new(PageInner {
                items: Vec::new(),
                total: 0,
                page: 1,
                size: 0,
                pages: 0,
                next_fetcher: None,
                transform: Arc::new(std::convert::identity),
            }),
        }
    }
}

impl<TRaw, TOut> Clone for Page<TRaw, TOut> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<TRaw: fmt::Debug, TOut> fmt::Debug for Page<TRaw, TOut> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Page")
            .field("items", &self.inner.items)
            .field("total", &self.inner.total)
            .field("page", &self.inner.page)
            .field("size", &self.inner.size)
            .field("pages", &self.inner.pages)
            .finish_non_exhaustive()
    }
}

impl<TRaw: PartialEq> PartialEq for Page<TRaw, TRaw> {
    fn eq(&self, other: &Self) -> bool {
        self.inner.items == other.inner.items
            && self.inner.total == other.inner.total
            && self.inner.page == other.inner.page
            && self.inner.size == other.inner.size
            && self.inner.pages == other.inner.pages
    }
}

impl<TRaw: Eq> Eq for Page<TRaw, TRaw> {}

impl<TRaw: serde::Serialize> serde::Serialize for Page<TRaw, TRaw> {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("Page", 5)?;
        s.serialize_field("items", &self.inner.items)?;
        s.serialize_field("total", &self.inner.total)?;
        s.serialize_field("page", &self.inner.page)?;
        s.serialize_field("size", &self.inner.size)?;
        s.serialize_field("pages", &self.inner.pages)?;
        s.end()
    }
}

impl<'de, TRaw: serde::Deserialize<'de> + 'static> serde::Deserialize<'de> for Page<TRaw, TRaw> {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let resp = PageResponse::<TRaw>::deserialize(deserializer)?;
        Ok(Self::from_page_response(resp))
    }
}

/// Build the `page` / `size` / `reverse` query parameters for a paginated POST
/// request. The returned pairs borrow the caller-owned numeric strings.
fn build_page_query<'a>(page: &'a str, size: &'a str, reverse: bool) -> Vec<(&'a str, &'a str)> {
    let mut query = vec![("page", page), ("size", size)];
    if reverse {
        query.push(("reverse", REVERSE_TRUE));
    }
    query
}

/// Paginate a POST endpoint that accepts `page` / `size` / `reverse` query
/// parameters and returns a [`PageResponse`].
///
/// The returned [`Page`] carries an attached fetcher so that
/// [`Page::next_page`] works automatically.
///
/// TODO(F4): reduce visibility to `pub(crate)` once the high-level
/// Honcho client exposes public paginated methods.
#[doc(hidden)]
pub async fn paginate_post<T>(
    http: &HttpClient,
    route: &str,
    body: Option<&serde_json::Value>,
    page: u64,
    size: u64,
    reverse: bool,
) -> Result<Page<T>>
where
    T: DeserializeOwned + Clone + Send + 'static,
{
    validate_pagination(page, size)?;

    // `size` and `reverse` are fixed for every page, so the size string is
    // computed once and shared with the per-page fetcher below.
    let size_str = size.to_string();

    let resp: PageResponse<T> = {
        let page_str = page.to_string();
        let query = build_page_query(&page_str, &size_str, reverse);
        http.post(route, body, &query).await?
    };

    let http_clone = http.clone();
    // O(1)-cloneable captures: `Arc<str>` for the route and `Arc<Value>` for the
    // request body, avoiding a deep clone of either on every page fetch.
    let route_arc: Arc<str> = Arc::from(route);
    let body_arc: Option<Arc<serde_json::Value>> = body.map(|b| Arc::new(b.clone()));

    Ok(resp.with_fetcher(move |page_num| {
        let http = http_clone.clone();
        let route = Arc::clone(&route_arc);
        let body = body_arc.clone();
        let size_str = size_str.clone();
        Box::pin(async move {
            let page_str = page_num.to_string();
            let query = build_page_query(&page_str, &size_str, reverse);
            let resp: PageResponse<T> = http.post(&route, body.as_deref(), &query).await?;
            Ok(resp)
        })
    }))
}

pub(crate) fn validate_pagination(page: u64, size: u64) -> Result<()> {
    if page == 0 {
        return Err(HonchoError::Validation(
            "page must be greater than or equal to 1".into(),
        ));
    }
    if !(1..=MAX_PAGE_SIZE).contains(&size) {
        return Err(HonchoError::Validation(
            "size must be between 1 and 100".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! `paginate_post` / `validate_pagination` smoke tests (formerly the
    //! `F3.4.3` section of `tests/pagination.rs`). Recovered inline because the
    //! cases build an [`HttpClient`] from the now-`pub(crate)` `http` module and
    //! can no longer live in an external test crate.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{Page, paginate_post};
    use crate::error::HonchoError;
    use crate::http::client::HttpClient;
    use crate::types::peer::Peer;

    fn peer_json(id: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "workspace_id": "ws1",
            "created_at": "2025-01-15T10:30:00Z",
            "metadata": {},
            "configuration": {}
        })
    }

    fn page_json(
        item_ids: &[&str],
        total: u64,
        page: u64,
        size: u64,
        pages: u64,
    ) -> serde_json::Value {
        serde_json::json!({
            "items": item_ids.iter().map(|id| peer_json(id)).collect::<Vec<_>>(),
            "total": total,
            "page": page,
            "size": size,
            "pages": pages
        })
    }

    // ═══════════════════════════════════════════════════════════════════════
    // F3.4.3 — paginate_post smoke tests
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn paginate_post_returns_first_page() {
        use wiremock::matchers::{body_json, method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let http = HttpClient::from_params(
            HttpClient::builder()
                .base_url(server.uri())
                .max_retries(0)
                .build(),
        )
        .unwrap();

        let page1_body = page_json(&["alice", "bob"], 5, 1, 2, 3);
        let request_body = serde_json::json!({"filter": true});

        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/peers/list"))
            .and(query_param("page", "1"))
            .and(query_param("size", "2"))
            .and(body_json(&request_body))
            .respond_with(ResponseTemplate::new(200).set_body_json(page1_body))
            .mount(&server)
            .await;

        let page: Page<Peer> = paginate_post(
            &http,
            "/v3/workspaces/ws1/peers/list",
            Some(&request_body),
            1,
            2,
            false,
        )
        .await
        .unwrap();

        assert_eq!(page.items().len(), 2);
        assert_eq!(page.total(), 5);
        assert_eq!(page.page(), 1);
        assert_eq!(page.pages(), 3);
        assert!(page.has_next());
    }

    #[tokio::test]
    async fn paginate_post_next_page_auto_fetches() {
        use wiremock::matchers::{body_json, method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let http = HttpClient::from_params(
            HttpClient::builder()
                .base_url(server.uri())
                .max_retries(0)
                .build(),
        )
        .unwrap();

        let page1_body = page_json(&["alice", "bob"], 5, 1, 2, 3);
        let page2_body = page_json(&["carol", "dave"], 5, 2, 2, 3);
        let request_body = serde_json::json!({});

        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/peers/list"))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page1_body))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/peers/list"))
            .and(query_param("page", "2"))
            .and(body_json(&request_body))
            .respond_with(ResponseTemplate::new(200).set_body_json(page2_body))
            .mount(&server)
            .await;

        let page1: Page<Peer> = paginate_post(
            &http,
            "/v3/workspaces/ws1/peers/list",
            Some(&request_body),
            1,
            2,
            false,
        )
        .await
        .unwrap();

        assert_eq!(page1.items()[0].id, "alice");

        let page2 = page1
            .next_page()
            .await
            .unwrap()
            .expect("page 2 should exist");
        assert_eq!(page2.items().len(), 2);
        assert_eq!(page2.page(), 2);
        assert_eq!(page2.items()[0].id, "carol");
        assert!(page2.has_next());
    }

    #[tokio::test]
    async fn paginate_post_with_reverse_param() {
        use wiremock::matchers::{method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let http = HttpClient::from_params(
            HttpClient::builder()
                .base_url(server.uri())
                .max_retries(0)
                .build(),
        )
        .unwrap();

        let page1_body = page_json(&["zoe"], 1, 1, 2, 1);

        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/peers/list"))
            .and(query_param("page", "1"))
            .and(query_param("size", "2"))
            .and(query_param("reverse", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page1_body))
            .mount(&server)
            .await;

        let page: Page<Peer> =
            paginate_post(&http, "/v3/workspaces/ws1/peers/list", None, 1, 2, true)
                .await
                .unwrap();

        assert_eq!(page.items()[0].id, "zoe");
        assert!(!page.has_next());
    }

    #[tokio::test]
    async fn paginate_post_sends_page_one_size_one_query() {
        use wiremock::matchers::{method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let http = HttpClient::from_params(
            HttpClient::builder()
                .base_url(server.uri())
                .max_retries(0)
                .build(),
        )
        .unwrap();

        let page_body = page_json(&["alice"], 1, 1, 1, 1);

        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/peers/list"))
            .and(query_param("page", "1"))
            .and(query_param("size", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page_body))
            .expect(1)
            .mount(&server)
            .await;

        let page: Page<Peer> =
            paginate_post(&http, "/v3/workspaces/ws1/peers/list", None, 1, 1, false)
                .await
                .unwrap();

        assert_eq!(page.items()[0].id, "alice");
        assert_eq!(page.page(), 1);
        assert_eq!(page.size(), 1);
    }

    #[tokio::test]
    async fn paginate_post_rejects_invalid_page_and_size_before_request() {
        use wiremock::MockServer;

        let server = MockServer::start().await;
        let http = HttpClient::from_params(
            HttpClient::builder()
                .base_url(server.uri())
                .max_retries(0)
                .build(),
        )
        .unwrap();

        for (page, size) in [(0, 50), (1, 0), (1, 101)] {
            let err = paginate_post::<Peer>(
                &http,
                "/v3/workspaces/ws1/peers/list",
                None,
                page,
                size,
                false,
            )
            .await
            .unwrap_err();
            assert!(matches!(err, HonchoError::Validation(_)));
        }

        let requests = server.received_requests().await.unwrap();
        assert!(
            requests.is_empty(),
            "invalid pagination should not send requests"
        );
    }

    #[tokio::test]
    async fn paginate_post_allows_large_page_and_size_100() {
        use wiremock::matchers::{method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let http = HttpClient::from_params(
            HttpClient::builder()
                .base_url(server.uri())
                .max_retries(0)
                .build(),
        )
        .unwrap();

        let page_body = page_json(&[], 0, 9999, 100, 0);

        Mock::given(method("POST"))
            .and(path("/v3/workspaces/ws1/peers/list"))
            .and(query_param("page", "9999"))
            .and(query_param("size", "100"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page_body))
            .expect(1)
            .mount(&server)
            .await;

        let page: Page<Peer> = paginate_post(
            &http,
            "/v3/workspaces/ws1/peers/list",
            None,
            9999,
            100,
            false,
        )
        .await
        .unwrap();

        assert_eq!(page.page(), 9999);
        assert_eq!(page.size(), 100);
    }
}
