use honcho_ai::Honcho;
use honcho_ai::types::session::SessionContextOptions;

use super::harness::TestReport;

#[allow(clippy::similar_names, clippy::too_many_lines)]
pub async fn run(honcho: &Honcho, report: &mut TestReport) {
    let peer = match honcho.peer("ctx-peer", None, None).await {
        Ok(p) => p,
        Err(e) => {
            report.fail("setup: create peer", &e.to_string());
            return;
        }
    };
    let session = match honcho.session("ctx-sess", None, None, None).await {
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
    let msg1 = match peer.message("Message 1 about Rust").build() {
        Ok(m) => m,
        Err(e) => {
            report.fail("setup: build messages", &e.to_string());
            return;
        }
    };
    let msg2 = match peer.message("Message 2 about async").build() {
        Ok(m) => m,
        Err(e) => {
            report.fail("setup: build messages", &e.to_string());
            return;
        }
    };
    let msg3 = match peer.message("Message 3 about testing").build() {
        Ok(m) => m,
        Err(e) => {
            report.fail("setup: build messages", &e.to_string());
            return;
        }
    };
    let msgs = match session.add_messages(vec![msg1, msg2, msg3]).await {
        Ok(m) => m,
        Err(e) => {
            report.fail("setup: add messages", &e.to_string());
            return;
        }
    };
    let _ = msgs;

    match session.context().await {
        Ok(ctx) => {
            report.pass("session_context_default");
            if ctx.messages.is_empty() {
                report.fail(
                    "session_context_default: messages",
                    "expected messages, got none",
                );
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

    {
        let opts = SessionContextOptions::builder()
            .peer_target("ctx-peer")
            .summary(true)
            .build();
        match opts.validate() {
            Ok(()) => match session.context_with_options(&opts).await {
                Ok(_ctx) => report.pass("session_context_with_peer_target"),
                Err(e) => report.fail("session_context_with_peer_target", &e.to_string()),
            },
            Err(e) => report.fail("session_context_with_peer_target: validate", &e.to_string()),
        }
    }

    {
        let opts = SessionContextOptions::builder()
            .peer_target("ctx-peer")
            .peer_perspective("ctx-peer")
            .build();
        match opts.validate() {
            Ok(()) => match session.context_with_options(&opts).await {
                Ok(_ctx) => report.pass("session_context_with_peer_perspective"),
                Err(e) => report.fail("session_context_with_peer_perspective", &e.to_string()),
            },
            Err(e) => report.fail(
                "session_context_with_peer_perspective: validate",
                &e.to_string(),
            ),
        }
    }

    {
        let opts = SessionContextOptions::builder()
            .peer_target("ctx-peer")
            .search_query("Rust")
            .build();
        match opts.validate() {
            Ok(()) => match session.context_with_options(&opts).await {
                Ok(_ctx) => report.pass("session_context_with_search"),
                Err(e) => report.fail("session_context_with_search", &e.to_string()),
            },
            Err(e) => report.fail("session_context_with_search: validate", &e.to_string()),
        }
    }

    {
        let opts = SessionContextOptions::builder().tokens(100).build();
        match session.context_with_options(&opts).await {
            Ok(_ctx) => report.pass("session_context_with_tokens"),
            Err(e) => report.fail("session_context_with_tokens", &e.to_string()),
        }
    }

    {
        let opts = SessionContextOptions::builder().max_conclusions(5).build();
        match opts.validate() {
            Ok(()) => match session.context_with_options(&opts).await {
                Ok(_ctx) => report.pass("session_context_with_max_conclusions"),
                Err(e) => report.fail("session_context_with_max_conclusions", &e.to_string()),
            },
            Err(e) => report.fail(
                "session_context_with_max_conclusions: validate",
                &e.to_string(),
            ),
        }
    }

    {
        let opts = SessionContextOptions::builder()
            .peer_target("ctx-peer")
            .search_top_k(10)
            .build();
        match opts.validate() {
            Ok(()) => match session.context_with_options(&opts).await {
                Ok(_ctx) => report.pass("session_context_with_search_top_k"),
                Err(e) => report.fail("session_context_with_search_top_k", &e.to_string()),
            },
            Err(e) => report.fail(
                "session_context_with_search_top_k: validate",
                &e.to_string(),
            ),
        }
    }

    {
        let opts = SessionContextOptions::builder()
            .peer_target("ctx-peer")
            .search_max_distance(0.5)
            .build();
        match opts.validate() {
            Ok(()) => match session.context_with_options(&opts).await {
                Ok(_ctx) => report.pass("session_context_with_search_max_distance"),
                Err(e) => report.fail("session_context_with_search_max_distance", &e.to_string()),
            },
            Err(e) => report.fail(
                "session_context_with_search_max_distance: validate",
                &e.to_string(),
            ),
        }
    }

    {
        let opts = SessionContextOptions::builder()
            .include_most_frequent(true)
            .build();
        match session.context_with_options(&opts).await {
            Ok(_ctx) => report.pass("session_context_include_most_frequent"),
            Err(e) => report.fail("session_context_include_most_frequent", &e.to_string()),
        }
    }

    {
        let opts = SessionContextOptions::builder().summary(true).build();
        match session.context_with_options(&opts).await {
            Ok(ctx) => {
                let _ = &ctx.messages;
                report.pass("context_messages_access");
            }
            Err(e) => report.fail("context_messages_access", &e.to_string()),
        }
    }

    {
        let opts = SessionContextOptions::builder().summary(true).build();
        match session.context_with_options(&opts).await {
            Ok(ctx) => {
                let _ = &ctx.summary;
                report.pass("context_summary_access");
            }
            Err(e) => report.fail("context_summary_access", &e.to_string()),
        }
    }

    {
        let opts = SessionContextOptions::builder().summary(true).build();
        match session.context_with_options(&opts).await {
            Ok(ctx) => {
                let openai = ctx.to_openai("assistant");
                if openai.is_empty() {
                    report.fail("context_to_openai", "returned empty vec");
                } else {
                    report.pass("context_to_openai");
                }
            }
            Err(e) => report.fail("context_to_openai", &e.to_string()),
        }
    }

    {
        let opts = SessionContextOptions::builder().summary(true).build();
        match session.context_with_options(&opts).await {
            Ok(ctx) => {
                let anthropic = ctx.to_anthropic("assistant");
                if anthropic.is_empty() {
                    report.fail("context_to_anthropic", "returned empty vec");
                } else {
                    report.pass("context_to_anthropic");
                }
            }
            Err(e) => report.fail("context_to_anthropic", &e.to_string()),
        }
    }

    {
        let opts = SessionContextOptions::builder().summary(true).build();
        match session.context_with_options(&opts).await {
            Ok(ctx) => {
                let len = ctx.len();
                let empty = ctx.is_empty();
                if len == 0 {
                    report.fail("context_len_and_is_empty", "len is 0");
                } else if empty {
                    report.fail("context_len_and_is_empty", "is_empty is true but len > 0");
                } else {
                    report.pass("context_len_and_is_empty");
                }
            }
            Err(e) => report.fail("context_len_and_is_empty", &e.to_string()),
        }
    }

    {
        let mut summaries_ok = false;
        for attempt in 0..5 {
            if attempt > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(500 * attempt)).await;
            }
            match session.summaries().await {
                Ok(summaries) => {
                    let has_short = summaries.short_summary.is_some();
                    let has_long = summaries.long_summary.is_some();
                    if has_short || has_long {
                        report.pass("session_summaries");
                        summaries_ok = true;
                        break;
                    }
                }
                Err(e) => {
                    report.fail("session_summaries", &e.to_string());
                    summaries_ok = true;
                    break;
                }
            }
        }
        if !summaries_ok {
            report.fail(
                "session_summaries",
                "no short or long summary after retries",
            );
        }
    }

    {
        let opts = SessionContextOptions::builder()
            .peer_target("ctx-peer")
            .summary(true)
            .build();
        match opts.validate() {
            Ok(()) => report.pass("validate_success"),
            Err(e) => report.fail("validate_success", &e.to_string()),
        }
    }

    {
        let opts = SessionContextOptions::builder()
            .peer_perspective("x")
            .build();
        match opts.validate() {
            Ok(()) => report.fail(
                "validate_peer_perspective_without_target",
                "expected validation error, got Ok",
            ),
            Err(_) => report.pass("validate_peer_perspective_without_target"),
        }
    }

    {
        let opts = SessionContextOptions::builder().search_query("x").build();
        match opts.validate() {
            Ok(()) => report.fail(
                "validate_search_without_target",
                "expected validation error, got Ok",
            ),
            Err(_) => report.pass("validate_search_without_target"),
        }
    }

    {
        let opts = SessionContextOptions::builder().search_top_k(200).build();
        match opts.validate() {
            Ok(()) => report.fail(
                "validate_search_top_k_range",
                "expected validation error, got Ok",
            ),
            Err(_) => report.pass("validate_search_top_k_range"),
        }
    }

    {
        let opts = SessionContextOptions::builder()
            .search_max_distance(2.0)
            .build();
        match opts.validate() {
            Ok(()) => report.fail(
                "validate_search_max_distance_range",
                "expected validation error, got Ok",
            ),
            Err(_) => report.pass("validate_search_max_distance_range"),
        }
    }

    {
        let opts = SessionContextOptions::builder()
            .max_conclusions(200)
            .build();
        match opts.validate() {
            Ok(()) => report.fail(
                "validate_max_conclusions_range",
                "expected validation error, got Ok",
            ),
            Err(_) => report.pass("validate_max_conclusions_range"),
        }
    }
}
