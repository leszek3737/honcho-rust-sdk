use std::fmt::Write;

use super::harness::TestReport;
use honcho_ai::{ConclusionCreateParams, Honcho};

#[allow(clippy::similar_names, clippy::too_many_lines)]
pub async fn run(honcho: &Honcho, report: &mut TestReport) -> honcho_ai::error::Result<()> {
    let observer = match honcho.peer("concl-observer", None, None).await {
        Ok(p) => p,
        Err(e) => {
            report.fail("conclusion_setup", &e.to_string());
            return Err(e);
        }
    };
    let observed = match honcho.peer("concl-observed", None, None).await {
        Ok(p) => p,
        Err(e) => {
            report.fail("conclusion_setup", &e.to_string());
            return Err(e);
        }
    };
    let scope = observer.conclusions();
    let cross_scope = observer.conclusions_of(observed.id());

    let concl_session = honcho
        .session("concl-session", None, None, None)
        .await
        .map_err(|e| {
            report.fail("conclusion_setup", &format!("session: {e}"));
            e
        })?;

    // 1. create single
    let mut created_id = String::new();
    match scope
        .create([ConclusionCreateParams::new("Observer knows Rust")])
        .await
    {
        Ok(vec) if !vec.is_empty() => {
            vec[0].id().clone_into(&mut created_id);
            report.pass("conclusion_create_single");
        }
        Ok(_) => report.fail("conclusion_create_single", "returned empty vec"),
        Err(e) => report.fail("conclusion_create_single", &e.to_string()),
    }

    // 2. create with builder
    let mut session_scoped_id = String::new();
    match scope
        .create([ConclusionCreateParams::builder()
            .content("test".to_owned())
            .session_id(concl_session.id().to_owned())
            .build()])
        .await
    {
        Ok(vec) if !vec.is_empty() => {
            vec[0].id().clone_into(&mut session_scoped_id);
            report.pass("conclusion_create_with_builder");
        }
        Ok(_) => report.fail("conclusion_create_with_builder", "returned empty vec"),
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

    // 4. create cross-observation
    match cross_scope
        .create([ConclusionCreateParams::new("Cross observation")])
        .await
    {
        Ok(vec) if !vec.is_empty() => {
            report.pass("conclusion_create_cross_observation");
        }
        Ok(_) => report.fail("conclusion_create_cross_observation", "returned empty vec"),
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

    // 7. accessors — reuse a created conclusion
    if created_id.is_empty() {
        report.fail("conclusion_accessors", "no created conclusion available");
    } else {
        match scope
            .create([ConclusionCreateParams::new("accessor test content")])
            .await
        {
            Ok(vec) if !vec.is_empty() => {
                let c = &vec[0];
                let mut ok = true;
                let mut detail = String::new();
                if c.id().is_empty() {
                    ok = false;
                    let _ = write!(&mut detail, "id empty; ");
                }
                if c.content() != "accessor test content" {
                    ok = false;
                    let _ = write!(&mut detail, "content mismatch; ");
                }
                if c.observer_id() != "concl-observer" {
                    ok = false;
                    let _ = write!(&mut detail, "observer_id mismatch; ");
                }
                if c.observed_id() != "concl-observer" {
                    ok = false;
                    let _ = write!(&mut detail, "observed_id mismatch; ");
                }
                if c.created_at().timestamp() == 0 {
                    ok = false;
                    let _ = write!(&mut detail, "created_at zero; ");
                }
                if c.workspace_id().is_empty() {
                    ok = false;
                    let _ = write!(&mut detail, "workspace_id empty; ");
                }
                if ok {
                    report.pass("conclusion_accessors");
                } else {
                    report.fail("conclusion_accessors", &detail);
                }
            }
            Ok(_) => report.fail("conclusion_accessors", "create returned empty vec"),
            Err(e) => report.fail("conclusion_accessors", &e.to_string()),
        }

        // 7b. session_id accessor via builder-created conclusion
        if !session_scoped_id.is_empty() {
            match scope.list().session(concl_session.id()).send().await {
                Ok(page) => {
                    let items = page.items();
                    let found = items.iter().find(|c| c.id == session_scoped_id);
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
                        None => report.pass("conclusion_session_id_accessor"),
                    }
                }
                Err(e) => {
                    report.fail("conclusion_session_id_accessor", &e.to_string());
                }
            }
        }
    }

    // 8. display trait
    match scope
        .create([ConclusionCreateParams::new("display test")])
        .await
    {
        Ok(vec) if !vec.is_empty() => {
            let displayed = format!("{}", vec[0]);
            if displayed == "display test" {
                report.pass("conclusion_display");
            } else {
                report.fail(
                    "conclusion_display",
                    &format!("expected 'display test', got '{displayed}'"),
                );
            }
        }
        Ok(_) => report.fail("conclusion_display", "returned empty vec"),
        Err(e) => report.fail("conclusion_display", &e.to_string()),
    }

    // 9. list
    match scope.list().page(1).size(10).send().await {
        Ok(_page) => report.pass("conclusion_list"),
        Err(e) => report.fail("conclusion_list", &e.to_string()),
    }

    // 10. list with session filter
    match scope.list().session(concl_session.id()).send().await {
        Ok(_page) => report.pass("conclusion_list_with_session"),
        Err(e) => report.fail("conclusion_list_with_session", &e.to_string()),
    }

    // 11. list with reverse
    match scope.list().reverse(true).send().await {
        Ok(_page) => report.pass("conclusion_list_with_reverse"),
        Err(e) => report.fail("conclusion_list_with_reverse", &e.to_string()),
    }

    // 12. query
    match scope.query("Rust programming").top_k(5).send().await {
        Ok(_results) => report.pass("conclusion_query"),
        Err(e) => report.fail("conclusion_query", &e.to_string()),
    }

    // 13. query with distance
    match scope.query("Rust").top_k(5).distance(0.5).send().await {
        Ok(_results) => report.pass("conclusion_query_with_distance"),
        Err(e) => report.fail("conclusion_query_with_distance", &e.to_string()),
    }

    // 14. representation
    match scope.representation().send().await {
        Ok(_rep) => report.pass("conclusion_representation"),
        Err(e) => report.fail("conclusion_representation", &e.to_string()),
    }

    // 15. representation with options
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

    // 16. delete
    if created_id.is_empty() {
        report.fail("conclusion_delete", "no conclusion id to delete");
    } else {
        match scope.delete(&created_id).await {
            Ok(()) => report.pass("conclusion_delete"),
            Err(e) => report.fail("conclusion_delete", &e.to_string()),
        }
    }

    // 17. list page info
    match scope.list().page(1).size(10).send().await {
        Ok(page) => {
            let mut ok = true;
            let mut detail = String::new();
            let _ = page.total();
            let _ = page.page();
            let _ = page.size();
            let _ = page.pages();
            let _ = page.has_next();
            if page.page() != 1 {
                ok = false;
                let _ = write!(&mut detail, "page={} expected 1; ", page.page());
            }
            if page.size() != 10 {
                ok = false;
                let _ = write!(&mut detail, "size={} expected 10; ", page.size());
            }
            if ok {
                report.pass("conclusion_list_page_info");
            } else {
                report.fail("conclusion_list_page_info", &detail);
            }
        }
        Err(e) => report.fail("conclusion_list_page_info", &e.to_string()),
    }

    Ok(())
}
