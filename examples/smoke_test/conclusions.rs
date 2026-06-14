use std::fmt::Write as _;

use super::harness::TestReport;
use honcho_ai::{Conclusion, ConclusionCreateParams, ConclusionScope, Honcho};

/// Create exactly one conclusion in `scope` with the given content and report
/// the outcome under `name`. Returns the created conclusion id on success so
/// callers can chain id-dependent assertions.
async fn create_one(
    scope: &ConclusionScope,
    name: &str,
    content: &str,
    report: &TestReport,
) -> Option<String> {
    match scope.create([ConclusionCreateParams::new(content)]).await {
        Ok(vec) => {
            if let Some(c) = vec.into_iter().next() {
                report.pass(name);
                Some(c.id().to_owned())
            } else {
                report.fail(name, "returned empty vec");
                None
            }
        }
        Err(e) => {
            report.fail(name, &e.to_string());
            None
        }
    }
}

#[allow(clippy::similar_names, clippy::too_many_lines)]
pub async fn run(honcho: &Honcho, report: &TestReport) -> honcho_ai::error::Result<()> {
    report.scenario("conclusions");

    // Setup failures are reported here via `report.fail` and the scenario
    // returns `Ok(())`; main does NOT additionally treat it as an abort, so a
    // setup failure is counted exactly once (and is RED).
    let observer = match honcho.peer("concl-observer").build().await {
        Ok(p) => p,
        Err(e) => {
            report.fail("conclusions setup", &e.to_string());
            return Ok(());
        }
    };
    let observed = match honcho.peer("concl-observed").build().await {
        Ok(p) => p,
        Err(e) => {
            report.fail("conclusions setup", &e.to_string());
            return Ok(());
        }
    };
    let scope = observer.conclusions();
    let cross_scope = observer.conclusions_of(observed.id());

    let concl_session = match honcho.session("concl-session").build().await {
        Ok(s) => s,
        Err(e) => {
            report.fail("conclusions setup", &format!("session: {e}"));
            return Ok(());
        }
    };

    // 1. create single
    let created_id = create_one(
        &scope,
        "conclusion_create_single",
        "Observer knows Rust",
        report,
    )
    .await;

    // 2. create with builder (session-scoped). Builder uses `on(String, into)`,
    // so `&str` is accepted without `.to_owned()`.
    let mut session_scoped_id: Option<String> = None;
    match scope
        .create([ConclusionCreateParams::builder()
            .content("test")
            .session_id(concl_session.id())
            .build()])
        .await
    {
        Ok(vec) => match vec.into_iter().next() {
            Some(c) => {
                session_scoped_id = Some(c.id().to_owned());
                report.pass("conclusion_create_with_builder");
            }
            None => report.fail("conclusion_create_with_builder", "returned empty vec"),
        },
        Err(e) => report.fail("conclusion_create_with_builder", &e.to_string()),
    }

    // 3. create batch
    match scope
        .create([
            ConclusionCreateParams::new("batch conclusion A"),
            ConclusionCreateParams::new("batch conclusion B"),
            ConclusionCreateParams::new("batch conclusion C"),
        ])
        .await
    {
        Ok(vec) if vec.len() == 3 => report.pass("conclusion_create_batch"),
        Ok(vec) => report.fail(
            "conclusion_create_batch",
            &format!("expected 3, got {}", vec.len()),
        ),
        Err(e) => report.fail("conclusion_create_batch", &e.to_string()),
    }

    // 4. create cross-observation — assert the auto-injected observer/observed
    // identities, the core `conclusions_of` property.
    match cross_scope
        .create([ConclusionCreateParams::new("Cross observation")])
        .await
    {
        Ok(vec) => match vec.first() {
            Some(c) if c.observer_id() == observer.id() && c.observed_id() == observed.id() => {
                report.pass("conclusion_create_cross_observation");
            }
            Some(c) => report.fail(
                "conclusion_create_cross_observation",
                &format!(
                    "expected observer={} observed={}, got observer={} observed={}",
                    observer.id(),
                    observed.id(),
                    c.observer_id(),
                    c.observed_id(),
                ),
            ),
            None => report.fail("conclusion_create_cross_observation", "returned empty vec"),
        },
        Err(e) => report.fail("conclusion_create_cross_observation", &e.to_string()),
    }

    // 5. scope observer_id
    match scope.observer_id() {
        "concl-observer" => report.pass("conclusion_scope_observer_id"),
        id => report.fail(
            "conclusion_scope_observer_id",
            &format!("expected concl-observer, got {id}"),
        ),
    }

    // 6. scope observed_id (self-scoped)
    match scope.observed_id() {
        "concl-observer" => report.pass("conclusion_scope_observed_id"),
        id => report.fail(
            "conclusion_scope_observed_id",
            &format!("expected concl-observer, got {id}"),
        ),
    }

    // 7. accessors — create a dedicated conclusion; this test does not depend
    // on test 1, so do not gate it on `created_id`.
    match scope
        .create([ConclusionCreateParams::new("accessor test content")])
        .await
    {
        Ok(vec) => match vec.first() {
            Some(c) => check_accessors(c, report),
            None => report.fail("conclusion_accessors", "create returned empty vec"),
        },
        Err(e) => report.fail("conclusion_accessors", &e.to_string()),
    }

    // 7b. session_id accessor — depends only on test 2 (the builder-created,
    // session-scoped conclusion), so gate it on `session_scoped_id` alone.
    if let Some(ref scoped_id) = session_scoped_id {
        match scope.list().session(concl_session.id()).send().await {
            Ok(page) => {
                let found = page.items().into_iter().find(|c| &c.id == scoped_id);
                match found {
                    Some(c) if c.session_id.as_deref() == Some(concl_session.id()) => {
                        report.pass("conclusion_session_id_accessor");
                    }
                    Some(c) => report.fail(
                        "conclusion_session_id_accessor",
                        &format!(
                            "expected session_id {}, got {:?}",
                            concl_session.id(),
                            c.session_id
                        ),
                    ),
                    None => report.fail(
                        "conclusion_session_id_accessor",
                        "session-scoped conclusion not found in session-filtered list",
                    ),
                }
            }
            Err(e) => report.fail("conclusion_session_id_accessor", &e.to_string()),
        }
    } else {
        report.fail(
            "conclusion_session_id_accessor",
            "no session-scoped conclusion available (test 2 failed)",
        );
    }

    // 8. display trait
    match scope
        .create([ConclusionCreateParams::new("display test")])
        .await
    {
        Ok(vec) => match vec.first() {
            Some(c) => {
                let displayed = c.to_string();
                if displayed == "display test" {
                    report.pass("conclusion_display");
                } else {
                    report.fail(
                        "conclusion_display",
                        &format!("expected 'display test', got '{displayed}'"),
                    );
                }
            }
            None => report.fail("conclusion_display", "returned empty vec"),
        },
        Err(e) => report.fail("conclusion_display", &e.to_string()),
    }

    // 9. list — assert the page reflects the conclusions created above.
    match scope.list().page(1).size(50).send().await {
        Ok(page) => {
            if page.items().is_empty() {
                report.fail("conclusion_list", "list returned no conclusions");
            } else {
                report.pass("conclusion_list");
            }
        }
        Err(e) => report.fail("conclusion_list", &e.to_string()),
    }

    // 10. list with session filter — the session-scoped conclusion must appear.
    match scope.list().session(concl_session.id()).send().await {
        Ok(page) => {
            let has_scoped = session_scoped_id
                .as_ref()
                .is_none_or(|id| page.items().iter().any(|c| &c.id == id));
            if has_scoped {
                report.pass("conclusion_list_with_session");
            } else {
                report.fail(
                    "conclusion_list_with_session",
                    "session-scoped conclusion missing from filtered list",
                );
            }
        }
        Err(e) => report.fail("conclusion_list_with_session", &e.to_string()),
    }

    // 11. list with reverse — when the page has >= 2 items, assert the order is
    // actually the reverse of the forward (non-reversed) list. With < 2 items
    // ordering is unobservable, so pass with a note.
    {
        let name = "conclusion_list_with_reverse";
        let forward = scope.list().reverse(false).page(1).size(50).send().await;
        let backward = scope.list().reverse(true).page(1).size(50).send().await;
        match (forward, backward) {
            (Ok(fwd), Ok(rev)) => {
                let fwd_ids: Vec<String> = fwd.items().into_iter().map(|c| c.id).collect();
                let rev_ids: Vec<String> = rev.items().into_iter().map(|c| c.id).collect();
                if rev_ids.len() >= 2 {
                    let mut expected = fwd_ids.clone();
                    expected.reverse();
                    if rev_ids == expected {
                        report.pass(name);
                    } else {
                        report.fail(
                            name,
                            &format!(
                                "reversed order {rev_ids:?} is not the reverse of forward {fwd_ids:?}"
                            ),
                        );
                    }
                } else {
                    // < 2 items: ordering cannot be observed, but the call itself
                    // succeeded, so this is a legitimate (weak) pass.
                    report.pass(name);
                }
            }
            (Err(e), _) | (_, Err(e)) => report.fail(name, &e.to_string()),
        }
    }

    // 12. query — assert the call succeeds and every returned item is
    // structurally well-formed (non-empty id/content). Non-emptiness cannot be
    // asserted: semantic search over embeddings may legitimately return no
    // matches for a brand-new workspace, so an empty result is not a failure.
    match scope.query("Rust programming").top_k(5).send().await {
        Ok(results) => {
            if let Some(bad) = results
                .iter()
                .find(|c| c.id().is_empty() || c.content().is_empty())
            {
                report.fail(
                    "conclusion_query",
                    &format!(
                        "malformed result: id={:?} content={:?}",
                        bad.id(),
                        bad.content()
                    ),
                );
            } else {
                report.pass("conclusion_query");
            }
        }
        Err(e) => report.fail("conclusion_query", &e.to_string()),
    }

    // 13. query with distance — `distance` is an input threshold, not a field on
    // the returned `Conclusion`, so it cannot be re-asserted from the response.
    // Assert the same structural invariant as test 12; non-emptiness is likewise
    // not guaranteed (embedding search may return nothing).
    match scope.query("Rust").top_k(5).distance(0.5).send().await {
        Ok(results) => {
            if let Some(bad) = results
                .iter()
                .find(|c| c.id().is_empty() || c.content().is_empty())
            {
                report.fail(
                    "conclusion_query_with_distance",
                    &format!(
                        "malformed result: id={:?} content={:?}",
                        bad.id(),
                        bad.content()
                    ),
                );
            } else {
                report.pass("conclusion_query_with_distance");
            }
        }
        Err(e) => report.fail("conclusion_query_with_distance", &e.to_string()),
    }

    // 14. representation — returns a `String`. A non-empty representation is the
    // structurally-correct shape, but it is LLM-generated and eventually
    // consistent, so an empty string for a fresh workspace is not a failure;
    // only an error fails. (Weak pass justified by eventual consistency.)
    match scope.representation().send().await {
        Ok(_rep) => report.pass("conclusion_representation"),
        Err(e) => report.fail("conclusion_representation", &e.to_string()),
    }

    // 15. representation with options — same eventual-consistency caveat as test
    // 14: the call must succeed, but a still-empty representation cannot be
    // asserted as non-empty without flake.
    match scope
        .representation()
        .search_query("test")
        .max_conclusions(20)
        .search_top_k(10)
        .send()
        .await
    {
        Ok(_rep) => report.pass("conclusion_representation_with_options"),
        Err(e) => report.fail("conclusion_representation_with_options", &e.to_string()),
    }

    // 16. delete — delete the single conclusion from test 1 and verify it is
    // gone, so repeated runs do not litter the workspace and a no-op delete is
    // caught.
    if let Some(id) = created_id {
        match scope.delete(&id).await {
            Ok(()) => match scope.list().page(1).size(100).send().await {
                Ok(page) => {
                    if page.items().iter().any(|c| c.id == id) {
                        report.fail("conclusion_delete", "conclusion still present after delete");
                    } else {
                        report.pass("conclusion_delete");
                    }
                }
                Err(e) => report.fail("conclusion_delete", &format!("verify list: {e}")),
            },
            Err(e) => report.fail("conclusion_delete", &e.to_string()),
        }
    } else {
        report.fail(
            "conclusion_delete",
            "no conclusion id to delete (test 1 failed)",
        );
    }

    // 17. list page info
    match scope.list().page(1).size(10).send().await {
        Ok(page) => {
            let mut detail = String::new();
            if page.page() != 1 {
                let _ = write!(detail, "page={} expected 1; ", page.page());
            }
            if page.size() != 10 {
                let _ = write!(detail, "size={} expected 10; ", page.size());
            }
            if detail.is_empty() {
                report.pass("conclusion_list_page_info");
            } else {
                report.fail("conclusion_list_page_info", &detail);
            }
        }
        Err(e) => report.fail("conclusion_list_page_info", &e.to_string()),
    }

    Ok(())
}

fn check_accessors(c: &Conclusion, report: &TestReport) {
    let name = "conclusion_accessors";
    let mut detail = String::new();
    if c.id().is_empty() {
        detail.push_str("id empty; ");
    }
    if c.content() != "accessor test content" {
        detail.push_str("content mismatch; ");
    }
    if c.observer_id() != "concl-observer" {
        detail.push_str("observer_id mismatch; ");
    }
    if c.observed_id() != "concl-observer" {
        detail.push_str("observed_id mismatch; ");
    }
    if c.created_at().timestamp() == 0 {
        detail.push_str("created_at zero; ");
    }
    if c.workspace_id().is_empty() {
        detail.push_str("workspace_id empty; ");
    }
    if detail.is_empty() {
        report.pass(name);
    } else {
        report.fail(name, &detail);
    }
}
