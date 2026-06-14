#![allow(
    // `common` is compiled into every integration-test crate, but no single
    // crate exercises every helper, so unused items are expected.
    dead_code,
    // Test helpers fail loud: a broken fixture or spec is a test bug, so we
    // `expect`/`panic!` with actionable messages instead of bubbling `Result`.
    clippy::expect_used,
    clippy::panic
)]

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use serde::Serialize;
use serde::de::DeserializeOwned;

static SCHEMAS: OnceLock<serde_json::Value> = OnceLock::new();

/// Absolute path to the crate root (`CARGO_MANIFEST_DIR`).
///
/// Used to locate test assets (fixtures, the `OpenAPI` spec) independently of the
/// current working directory.
fn manifest_dir() -> PathBuf {
    let dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    PathBuf::from(dir)
}

fn openapi_spec() -> &'static serde_json::Value {
    SCHEMAS.get_or_init(|| {
        let path = std::env::var("HONCHO_OPENAPI_SPEC").map_or_else(
            |_| manifest_dir().join("tests/schemas/openapi.json"),
            PathBuf::from,
        );
        let content = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read openapi.json at {}: {e}", path.display()));
        serde_json::from_str(&content)
            .unwrap_or_else(|e| panic!("failed to parse openapi.json: {e}"))
    })
}

fn schema_by_name(name: &str) -> serde_json::Value {
    let spec = openapi_spec();
    let schemas = spec["components"]["schemas"]
        .as_object()
        .unwrap_or_else(|| panic!("openapi spec has no components.schemas"));
    schemas
        .get(name)
        .unwrap_or_else(|| panic!("schema {name} not found in OpenAPI spec"))
        .clone()
}

/// Recursively inlines `$ref` pointers (`#/components/schemas/<Name>`) into a
/// self-contained schema that `jsonschema` can compile.
///
/// `active` tracks the schema names on the current resolution stack so a
/// cyclic reference panics with a clear message instead of recursing until the
/// thread stack overflows. Names are removed on the way back up, so a schema
/// referenced twice in sibling positions (a non-cyclic diamond) is fine.
fn resolve_refs(
    value: &serde_json::Value,
    spec: &serde_json::Value,
    active: &mut HashSet<String>,
) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(ref_path) = map.get("$ref").and_then(|v| v.as_str()) {
                let schema_name = ref_path
                    .rsplit('/')
                    .next()
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| panic!("malformed $ref (empty schema name): {ref_path}"));
                assert!(
                    !active.contains(schema_name),
                    "cyclic $ref detected: {schema_name}"
                );
                let resolved = spec["components"]["schemas"]
                    .get(schema_name)
                    .unwrap_or_else(|| {
                        panic!("unresolved $ref: {schema_name} (not found in components.schemas)")
                    })
                    .clone();
                active.insert(schema_name.to_string());
                let result = resolve_refs(&resolved, spec, active);
                active.remove(schema_name);
                return result;
            }
            let resolved: serde_json::Map<String, serde_json::Value> = map
                .iter()
                .map(|(k, v)| (k.clone(), resolve_refs(v, spec, active)))
                .collect();
            serde_json::Value::Object(resolved)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(|v| resolve_refs(v, spec, active)).collect())
        }
        other => other.clone(),
    }
}

/// Returns the compiled validator for an `OpenAPI` component schema, compiling it
/// on first use and caching it for subsequent calls.
///
/// `jsonschema::validator_for` re-parses and re-compiles the whole schema on
/// every call, so without this cache each fixture test would recompile its
/// schema from scratch. Validators are shared via `Arc` and validation happens
/// outside the lock.
fn compiled_validator(schema_name: &str) -> Arc<jsonschema::Validator> {
    static VALIDATORS: OnceLock<Mutex<HashMap<String, Arc<jsonschema::Validator>>>> =
        OnceLock::new();
    let cache = VALIDATORS.get_or_init(|| Mutex::new(HashMap::new()));

    // Fast path: take the lock only to read the cache, then release it before
    // the expensive resolve + compile so threads don't serialize on compilation.
    {
        let guard = cache
            .lock()
            .unwrap_or_else(|e| panic!("validator cache mutex poisoned: {e}"));
        if let Some(validator) = guard.get(schema_name) {
            return Arc::clone(validator);
        }
    } // guard dropped here: a panic in resolve/compile below cannot poison it

    // Compile without holding the lock. Two threads racing on the same unseen
    // schema may both compile it (rare, one-time); `or_insert` keeps a single
    // cache entry and discards the redundant `Arc`.
    let spec = openapi_spec();
    let schema = schema_by_name(schema_name);
    let resolved = resolve_refs(&schema, spec, &mut HashSet::new());
    let compiled = jsonschema::validator_for(&resolved)
        .unwrap_or_else(|e| panic!("failed to compile schema {schema_name}: {e}"));
    let validator = Arc::new(compiled);

    // Re-acquire the lock and publish via entry API.
    let mut guard = cache
        .lock()
        .unwrap_or_else(|e| panic!("validator cache mutex poisoned: {e}"));
    guard
        .entry(schema_name.to_string())
        .or_insert(validator)
        .clone()
}

/// Loads a JSON test fixture from `tests/fixtures/<name>/<variant>.json`.
///
/// Panics with an actionable message if the file is missing or is not valid
/// JSON.
pub fn load_fixture(name: &str, variant: &str) -> serde_json::Value {
    let mut path = manifest_dir();
    path.push("tests/fixtures");
    path.push(name);
    path.push(format!("{variant}.json"));
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to load fixture {name}/{variant}.json: {e}"));
    serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("failed to parse fixture {name}/{variant}.json: {e}"))
}

/// Validates `value` against the named `OpenAPI` component schema.
///
/// Callers pass the SDK's own serialized output (`&to_value(&deserialized)`),
/// not the raw input fixture, so this asserts that what the SDK *produces*
/// still conforms to the published schema. Panics listing every validation
/// error on failure.
pub fn validate_openapi(value: &serde_json::Value, schema_name: &str) {
    let validator = compiled_validator(schema_name);
    let errors: Vec<String> = validator
        .iter_errors(value)
        .map(|e| e.to_string())
        .collect();
    assert!(
        errors.is_empty(),
        "OpenAPI validation failed for schema {schema_name}:\n  {}",
        errors.join("\n  ")
    );
}

/// Asserts that `T` survives a JSON round-trip without losing or renaming data.
///
/// Strict fidelity: after deserializing the fixture into `T` and serializing it
/// back, the SDK output must equal the input fixture (modulo key order). This
/// catches fields silently dropped by `#[serde(skip)]`, renamed keys, or
/// defaulted-away values.
// `fixture` is taken by value to keep all 47 call-sites unchanged; it is cloned
// for deserialization and borrowed for the fidelity check, never consumed.
#[allow(clippy::needless_pass_by_value)]
pub fn roundtrip<T>(fixture: serde_json::Value)
where
    T: Serialize + DeserializeOwned,
{
    let deserialized: T = serde_json::from_value(fixture.clone())
        .unwrap_or_else(|e| panic!("deserialize failed for {}: {e}", std::any::type_name::<T>()));

    let serialized = serde_json::to_value(&deserialized)
        .unwrap_or_else(|e| panic!("serialize failed for {}: {e}", std::any::type_name::<T>()));

    // Strict fidelity: SDK output must equal the input fixture.
    assert_eq!(
        canonicalize(&fixture),
        canonicalize(&serialized),
        "lossy roundtrip (field dropped by {}): SDK output != input fixture",
        std::any::type_name::<T>()
    );
}

fn canonicalize(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut sorted: serde_json::Map<String, serde_json::Value> = map
                .iter()
                .map(|(k, v)| (k.clone(), canonicalize(v)))
                .collect();
            sorted.sort_keys();
            serde_json::Value::Object(sorted)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(canonicalize).collect())
        }
        other => other.clone(),
    }
}

// ════════════════════════════════════════════════════════════════════════
// Wiremock test helpers
//
// Shared HTTP-mock fixtures for the wiremock-based unit tests (Peer, Session,
// client, pagination). These factor out the `reqwest::Client::new()`,
// `make_honcho`, response-builder, and pagination-fetcher boilerplate that was
// previously copy-pasted across ~10 test files.
//
// Everything below is independent of the OpenAPI schema helpers above; the two
// sections share only the crate-level `#![allow(dead_code, …)]`.
// ════════════════════════════════════════════════════════════════════════

use std::future::Future;
use std::pin::Pin;

use honcho_ai::Honcho;
use honcho_ai::error::HonchoError;
use honcho_ai::types::pagination::PageResponse;
use honcho_ai::types::peer::Peer;
use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Default workspace id used by the wiremock test fixtures.
///
/// Chosen because the existing suite already standardizes on `"ws1"` in every
/// mocked path and request body, so adopting helpers requires no churn.
pub const TEST_WORKSPACE_ID: &str = "ws1";

/// Fixed API key injected into the test client. The mock server never checks
/// authorization, so the value only has to be non-empty and stable.
const TEST_API_KEY: &str = "test-api-key";

/// Fixed `created_at` timestamp (RFC 3339) used by every response builder.
///
/// `created_at` deserializes into `chrono::DateTime<Utc>`, so it must be a
/// valid RFC 3339 instant.
const TEST_CREATED_AT: &str = "2025-01-15T10:30:00Z";

/// Returns the process-wide shared `reqwest::Client`, built once.
///
/// `reqwest::Client` is internally reference-counted, so cloning is cheap and
/// every test shares one connection pool instead of constructing a fresh client
/// per request (the `reqwest::Client::new()` duplication the audit flagged).
pub fn http_client() -> reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new).clone()
}

/// Builds a [`Honcho`] pointed at `base_url` for the default test workspace
/// ([`TEST_WORKSPACE_ID`]), with a fixed API key and default retry policy.
///
/// Panics if construction fails (a broken test setup, never a runtime path).
pub fn make_honcho(base_url: &str) -> Honcho {
    make_honcho_with(base_url, TEST_WORKSPACE_ID)
}

/// Like [`make_honcho`] but with retries disabled (`max_retries(0)`).
///
/// Error-path tests assert that a single 5xx surfaces after exactly one attempt.
/// With the default retry policy an idempotent verb would be retried (sleeping
/// through backoff) before the error is observed, making both the call count and
/// the timing non-deterministic. Disabling retries keeps those tests fast and
/// their `.expect(1)` mock assertions exact.
///
/// Panics if construction fails (a broken test setup, never a runtime path).
pub fn make_honcho_no_retry(base_url: &str) -> Honcho {
    Honcho::from_params(
        Honcho::builder()
            .base_url(base_url)
            .workspace_id(TEST_WORKSPACE_ID)
            .api_key(TEST_API_KEY)
            .max_retries(0)
            .build(),
    )
    .expect("construct no-retry Honcho test client")
}

/// Like [`make_honcho`] but with an explicit `workspace_id`, for tests that
/// exercise more than one workspace.
pub fn make_honcho_with(base_url: &str, workspace_id: &str) -> Honcho {
    Honcho::from_params(
        Honcho::builder()
            .base_url(base_url)
            .workspace_id(workspace_id)
            .api_key(TEST_API_KEY)
            .build(),
    )
    .expect("construct Honcho test client")
}

/// JSON body for a `Workspace` response (`POST /v3/workspaces` and friends).
pub fn workspace_response(id: &str) -> serde_json::Value {
    json!({
        "id": id,
        "metadata": {},
        "configuration": {},
        "created_at": TEST_CREATED_AT
    })
}

/// JSON body for a `Peer` response (peer get-or-create / refresh).
pub fn peer_response(id: &str) -> serde_json::Value {
    json!({
        "id": id,
        "workspace_id": TEST_WORKSPACE_ID,
        "created_at": TEST_CREATED_AT,
        "metadata": {},
        "configuration": {}
    })
}

/// JSON body for a `SessionResponse` (session get-or-create / refresh).
pub fn session_response(id: &str) -> serde_json::Value {
    json!({
        "id": id,
        "is_active": true,
        "workspace_id": TEST_WORKSPACE_ID,
        "metadata": {},
        "configuration": {},
        "created_at": TEST_CREATED_AT
    })
}

/// Builds the SDK's paginated wire shape (`Page` / `PageResponse`) wrapping the
/// already-serialized `items`.
///
/// `items` is taken by value so call sites can hand over an owned `Vec` without
/// an extra clone.
#[allow(clippy::needless_pass_by_value)]
pub fn page_json(
    items: Vec<serde_json::Value>,
    total: u64,
    page: u64,
    size: u64,
    pages: u64,
) -> serde_json::Value {
    json!({
        "items": items,
        "total": total,
        "page": page,
        "size": size,
        "pages": pages
    })
}

/// Returns an *un-mounted* `Mock` for the workspace-ensure POST.
///
/// Matches `POST /v3/workspaces` with body `{"id": workspace_id}` and responds
/// `200` + [`workspace_response`]. The caller decides the call-count
/// expectation (`.expect(..)` / `.up_to_n_times(..)`) and mounts it:
///
/// ```ignore
/// workspace_ensure_mock("ws1").expect(1).mount(&server).await;
/// ```
pub fn workspace_ensure_mock(workspace_id: &str) -> Mock {
    Mock::given(method("POST"))
        .and(path("/v3/workspaces"))
        .and(body_json(json!({ "id": workspace_id })))
        .respond_with(ResponseTemplate::new(200).set_body_json(workspace_response(workspace_id)))
}

/// Mounts the workspace-ensure POST for [`TEST_WORKSPACE_ID`] with an explicit
/// call-count expectation that wiremock verifies on server drop.
///
/// Convenience wrapper around [`workspace_ensure_mock`] for the common
/// single-workspace flow.
pub async fn mount_workspace_ensure(server: &MockServer, expect: u64) {
    workspace_ensure_mock(TEST_WORKSPACE_ID)
        .expect(expect)
        .mount(server)
        .await;
}

/// Builds the next-page fetcher closure used to drive `Page::next_page` /
/// `Page::into_stream` against a mock server.
///
/// This lifts the ~15-line `with_fetcher` closure duplicated across the
/// pagination tests: it POSTs to `base_url + list_path` with the `page`/`size`
/// query params and the captured JSON `body`, then deserializes the response
/// into `PageResponse<Peer>`. Adopt it with:
///
/// ```ignore
/// let page = page1_resp.with_fetcher(peer_fetcher(
///     &server.uri(),
///     "/v3/workspaces/ws1/peers/list",
///     2,
///     serde_json::json!({}),
/// ));
/// ```
#[allow(clippy::needless_pass_by_value)]
pub fn peer_fetcher(
    base_url: &str,
    list_path: &str,
    size: u64,
    body: serde_json::Value,
) -> impl Fn(u64) -> Pin<Box<dyn Future<Output = honcho_ai::error::Result<PageResponse<Peer>>> + Send>>
+ use<> {
    // `+ use<>` opts the returned `impl Fn` out of capturing *any* generic or
    // lifetime parameter (precise capturing, edition 2024). The closure body
    // below `.to_owned()`s `base_url` and `list_path` into owned `String`s and
    // clones `body`, so it borrows nothing from the arguments and the returned
    // type is genuinely `'static`. Without `use<>` RPIT would capture the input
    // `&str` lifetimes, forcing callers to hand in `'static` strings (e.g. by
    // leaking `server.uri()`); `use<>` removes that artificial requirement.
    let base = base_url.to_owned();
    let list_path = list_path.to_owned();
    move |page_num: u64| {
        let url = format!("{base}{list_path}");
        let body = body.clone();
        Box::pin(async move {
            let response = http_client()
                .post(url)
                .query(&[("page", page_num)])
                .query(&[("size", size)])
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await
                .map_err(HonchoError::Transport)?;
            let pr: PageResponse<Peer> = response.json().await.map_err(HonchoError::Transport)?;
            Ok(pr)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A self-referential `$ref` chain must panic with a clear message rather
    /// than recursing until the stack overflows.
    #[test]
    #[should_panic(expected = "cyclic $ref detected")]
    fn resolve_refs_detects_cycle() {
        let spec = serde_json::json!({
            "components": {
                "schemas": {
                    "A": { "properties": { "b": { "$ref": "#/components/schemas/B" } } },
                    "B": { "properties": { "a": { "$ref": "#/components/schemas/A" } } }
                }
            }
        });
        let schema = spec["components"]["schemas"]["A"].clone();
        let _ = resolve_refs(&schema, &spec, &mut HashSet::new());
    }
}
