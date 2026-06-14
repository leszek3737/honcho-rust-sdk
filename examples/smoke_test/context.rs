use honcho_ai::Honcho;
use honcho_ai::error::HonchoError;
use honcho_ai::types::session::SessionContextOptions;

use super::harness::TestReport;

/// Base back-off (ms) for eventually-consistent retries (summaries).
const RETRY_BASE_MS: u64 = 500;

#[allow(clippy::similar_names, clippy::too_many_lines)]
pub async fn run(honcho: &Honcho, report: &TestReport) {
    report.scenario("context");

    let peer = match honcho.peer("ctx-peer").build().await {
        Ok(p) => p,
        Err(e) => {
            report.fail("setup: create peer", &e.to_string());
            return;
        }
    };
    let session = match honcho.session("ctx-sess").build().await {
        Ok(s) => s,
        Err(e) => {
            report.fail("setup: create session", &e.to_string());
            return;
        }
    };
    if let Err(e) = session.add_peer(peer.id()).await {
        report.fail("setup: add peer", &e.to_string());
        return;
    }

    // Build the seed messages with a loop + collect into a single Result.
    let contents = [
        "Message 1 about Rust",
        "Message 2 about async",
        "Message 3 about testing",
    ];
    let built: Result<Vec<_>, _> = contents.iter().map(|c| peer.message(*c).build()).collect();
    let messages = match built {
        Ok(m) => m,
        Err(e) => {
            report.fail("setup: build messages", &e.to_string());
            return;
        }
    };
    let added = match session.add_messages(messages).await {
        Ok(m) => m,
        Err(e) => {
            report.fail("setup: add messages", &e.to_string());
            return;
        }
    };
    if added.len() == 3 {
        report.pass("setup_add_messages");
    } else {
        report.fail(
            "setup_add_messages",
            &format!("expected 3, got {}", added.len()),
        );
    }

    // session.context() — assert on the messages before reporting pass.
    match session.context().await {
        Ok(ctx) => {
            if ctx.messages.is_empty() {
                report.fail("session_context_default", "expected messages, got none");
            } else {
                report.pass("session_context_default");
            }
        }
        Err(e) => report.fail("session_context_default", &e.to_string()),
    }

    let opts = SessionContextOptions::builder().summary(true).build();
    match session.context_with_options(&opts).await {
        Ok(_ctx) => report.pass("session_context_with_summary"),
        Err(e) => report.fail("session_context_with_summary", &e.to_string()),
    }

    let opts = SessionContextOptions::builder().summary(false).build();
    match session.context_with_options(&opts).await {
        Ok(_ctx) => report.pass("session_context_no_summary"),
        Err(e) => report.fail("session_context_no_summary", &e.to_string()),
    }

    let opts = SessionContextOptions::builder()
        .limit_to_session(true)
        .build();
    match session.context_with_options(&opts).await {
        Ok(_ctx) => report.pass("session_context_limit_to_session"),
        Err(e) => report.fail("session_context_limit_to_session", &e.to_string()),
    }

    // `context_with_options` validates internally, so manual `opts.validate()`
    // before the call is redundant — call directly.
    let opts = SessionContextOptions::builder()
        .peer_target("ctx-peer")
        .summary(true)
        .build();
    match session.context_with_options(&opts).await {
        Ok(_ctx) => report.pass("session_context_with_peer_target"),
        Err(e) => report.fail("session_context_with_peer_target", &e.to_string()),
    }

    let opts = SessionContextOptions::builder()
        .peer_target("ctx-peer")
        .peer_perspective("ctx-peer")
        .build();
    match session.context_with_options(&opts).await {
        Ok(_ctx) => report.pass("session_context_with_peer_perspective"),
        Err(e) => report.fail("session_context_with_peer_perspective", &e.to_string()),
    }

    let opts = SessionContextOptions::builder()
        .peer_target("ctx-peer")
        .search_query("Rust")
        .build();
    match session.context_with_options(&opts).await {
        Ok(_ctx) => report.pass("session_context_with_search"),
        Err(e) => report.fail("session_context_with_search", &e.to_string()),
    }

    let opts = SessionContextOptions::builder().tokens(100).build();
    match session.context_with_options(&opts).await {
        Ok(_ctx) => report.pass("session_context_with_tokens"),
        Err(e) => report.fail("session_context_with_tokens", &e.to_string()),
    }

    let opts = SessionContextOptions::builder().max_conclusions(5).build();
    match session.context_with_options(&opts).await {
        Ok(_ctx) => report.pass("session_context_with_max_conclusions"),
        Err(e) => report.fail("session_context_with_max_conclusions", &e.to_string()),
    }

    let opts = SessionContextOptions::builder()
        .peer_target("ctx-peer")
        .search_top_k(10)
        .build();
    match session.context_with_options(&opts).await {
        Ok(_ctx) => report.pass("session_context_with_search_top_k"),
        Err(e) => report.fail("session_context_with_search_top_k", &e.to_string()),
    }

    let opts = SessionContextOptions::builder()
        .peer_target("ctx-peer")
        .search_max_distance(0.5)
        .build();
    match session.context_with_options(&opts).await {
        Ok(_ctx) => report.pass("session_context_with_search_max_distance"),
        Err(e) => report.fail("session_context_with_search_max_distance", &e.to_string()),
    }

    let opts = SessionContextOptions::builder()
        .include_most_frequent(true)
        .build();
    match session.context_with_options(&opts).await {
        Ok(_ctx) => report.pass("session_context_include_most_frequent"),
        Err(e) => report.fail("session_context_include_most_frequent", &e.to_string()),
    }

    // Consolidate the formerly-five identical `summary(true)` fetches into a
    // single fetch shared by all downstream assertions.
    let opts = SessionContextOptions::builder().summary(true).build();
    match session.context_with_options(&opts).await {
        Ok(ctx) => {
            // messages access
            if ctx.messages.is_empty() {
                report.fail("context_messages_access", "expected messages, got none");
            } else {
                report.pass("context_messages_access");
            }

            // summary access (compile-time field guarantee; just exercise it)
            let _ = &ctx.summary;
            report.pass("context_summary_access");

            // to_openai with the peer that actually authored the messages, so
            // the `role: "assistant"` branch is exercised.
            let openai = ctx.to_openai("ctx-peer");
            if openai.is_empty() {
                report.fail("context_to_openai", "returned empty vec");
            } else {
                report.pass("context_to_openai");
            }

            // to_anthropic similarly.
            let anthropic = ctx.to_anthropic("ctx-peer");
            if anthropic.is_empty() {
                report.fail("context_to_anthropic", "returned empty vec");
            } else {
                report.pass("context_to_anthropic");
            }

            // len()/is_empty() cross-check against the messages vector.
            let len = ctx.len();
            if len == 0 {
                report.fail("context_len_and_is_empty", "len is 0");
            } else if ctx.is_empty() {
                report.fail("context_len_and_is_empty", "is_empty true but len > 0");
            } else if len < ctx.messages.len() {
                // `len()` counts messages PLUS summary/peer_representation/peer_card
                // overhead, and this request sets `.summary(true)`, so on a correct
                // SDK `len() >= messages.len()`. Only a STRICTLY smaller len is a bug.
                report.fail(
                    "context_len_and_is_empty",
                    &format!("len() {} < messages.len() {}", len, ctx.messages.len()),
                );
            } else {
                report.pass("context_len_and_is_empty");
            }
        }
        Err(e) => {
            for name in [
                "context_messages_access",
                "context_summary_access",
                "context_to_openai",
                "context_to_anthropic",
                "context_len_and_is_empty",
            ] {
                report.fail(name, &e.to_string());
            }
        }
    }

    // summaries — LLM-generated and eventually consistent. Retry on both empty
    // results *and* transient errors; only a persistent error fails.
    {
        let mut last_err: Option<String> = None;
        let mut done = false;
        for attempt in 0..5 {
            if attempt > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(RETRY_BASE_MS * attempt)).await;
            }
            match session.summaries().await {
                Ok(summaries) => {
                    if summaries.short_summary.is_some() || summaries.long_summary.is_some() {
                        report.pass("session_summaries");
                        done = true;
                        break;
                    }
                }
                Err(e) => last_err = Some(e.to_string()),
            }
        }
        if !done {
            match last_err {
                Some(e) => report.fail("session_summaries", &format!("persistent error: {e}")),
                None => report.fail(
                    "session_summaries",
                    "no short or long summary after retries (LLM may be slow)",
                ),
            }
        }
    }

    // Positive validation.
    let opts = SessionContextOptions::builder()
        .peer_target("ctx-peer")
        .summary(true)
        .build();
    match opts.validate() {
        Ok(()) => report.pass("validate_success"),
        Err(e) => report.fail("validate_success", &e.to_string()),
    }

    // Negative validation — must be a `Validation` error specifically, not any
    // error, and the message must mention the offending field.
    check_validation_error(
        &SessionContextOptions::builder()
            .peer_perspective("x")
            .build(),
        "validate_peer_perspective_without_target",
        "perspective",
        report,
    );
    check_validation_error(
        &SessionContextOptions::builder().search_query("x").build(),
        "validate_search_without_target",
        "search",
        report,
    );
    check_validation_error(
        &SessionContextOptions::builder().search_top_k(200).build(),
        "validate_search_top_k_range",
        "top_k",
        report,
    );
    check_validation_error(
        &SessionContextOptions::builder().search_top_k(0).build(),
        "validate_search_top_k_zero",
        "top_k",
        report,
    );
    check_validation_error(
        &SessionContextOptions::builder()
            .search_max_distance(2.0)
            .build(),
        "validate_search_max_distance_range",
        "distance",
        report,
    );
    check_validation_error(
        &SessionContextOptions::builder()
            .search_max_distance(-0.1)
            .build(),
        "validate_search_max_distance_negative",
        "distance",
        report,
    );
    check_validation_error(
        &SessionContextOptions::builder()
            .max_conclusions(200)
            .build(),
        "validate_max_conclusions_range",
        "conclusions",
        report,
    );
    check_validation_error(
        &SessionContextOptions::builder().max_conclusions(0).build(),
        "validate_max_conclusions_zero",
        "conclusions",
        report,
    );
    check_validation_error(
        &SessionContextOptions::builder().tokens(0).build(),
        "validate_tokens_zero",
        "token",
        report,
    );
}

/// Assert that `opts.validate()` fails specifically with
/// [`HonchoError::Validation`] whose message contains `needle` (case-insensitive).
fn check_validation_error(
    opts: &SessionContextOptions,
    name: &str,
    needle: &str,
    report: &TestReport,
) {
    match opts.validate() {
        Ok(()) => report.fail(name, "expected validation error, got Ok"),
        Err(HonchoError::Validation(msg)) => {
            if msg.to_lowercase().contains(&needle.to_lowercase()) {
                report.pass(name);
            } else {
                report.fail(
                    name,
                    &format!("validation message {msg:?} does not mention {needle:?}"),
                );
            }
        }
        Err(other) => report.fail(name, &format!("expected Validation error, got {other}")),
    }
}
