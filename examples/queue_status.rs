#![allow(clippy::print_stdout)]
//! Queue status: check background processing progress.
//!
//! Demonstrates checking the deriver queue at both the workspace
//! and session levels, polling until the queue drains, and scheduling
//! a dream task.
//!
//! Run with `cargo run --example queue_status`

use std::time::Duration;

use honcho_ai::Honcho;

/// Print the four work-unit counters under a label, deduplicating the
/// otherwise-identical workspace, session, and per-session status prints.
/// Takes raw counters so it works for both `QueueStatus` and
/// `SessionQueueStatus`, which share the same four fields.
fn print_status(label: &str, total: u64, pending: u64, in_progress: u64, completed: u64) {
    println!(
        "{label}: total={total}, pending={pending}, in_progress={in_progress}, completed={completed}",
    );
}

#[tokio::main]
async fn main() -> honcho_ai::error::Result<()> {
    let honcho = Honcho::new("http://localhost:8000", "queue-demo")?;

    let peer = honcho.peer("observer-1").build().await?;
    let session = honcho.session("sess-1").build().await?;

    session
        .add_messages(vec![peer.message("Trigger some background work").build()?])
        .await?;

    // Poll the workspace queue until it drains. Reading once right after
    // add_messages usually shows stale/zero counters because derivation is
    // asynchronous, so loop until pending_work_units == 0 (capped so the
    // example never hangs against an idle or unreachable backend).
    for attempt in 0..10 {
        // Filters: observer_id, sender_id, session_id — all None = whole workspace.
        let ws_status = honcho.queue_status(None, None, None).await?;
        print_status(
            &format!("Workspace queue (attempt {attempt})"),
            ws_status.total_work_units,
            ws_status.pending_work_units,
            ws_status.in_progress_work_units,
            ws_status.completed_work_units,
        );
        if ws_status.pending_work_units == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    // Filters: sender_id, session_id — None here scopes to the whole session.
    let sess_status = session.queue_status(None, None).await?;
    print_status(
        "Session queue",
        sess_status.total_work_units,
        sess_status.pending_work_units,
        sess_status.in_progress_work_units,
        sess_status.completed_work_units,
    );

    // Per-session breakdown is only populated when not filtered by session
    // (i.e. on the workspace-level query above). Re-read it for the report.
    // Filters: observer_id, sender_id, session_id.
    let ws_final = honcho.queue_status(None, None, None).await?;
    if let Some(sessions) = &ws_final.sessions {
        println!("Per-session breakdown ({} session(s)):", sessions.len());
        for (id, status) in sessions {
            print_status(
                &format!("  session {id}"),
                status.total_work_units,
                status.pending_work_units,
                status.in_progress_work_units,
                status.completed_work_units,
            );
        }
    } else {
        println!("No per-session breakdown available");
    }

    // schedule_dream(observer, session_id, observed_peer): reuse peer.id()
    // instead of repeating the literal, so the dream targets the peer we
    // actually created.
    honcho.schedule_dream(peer.id(), None, None).await?;
    println!("Dream scheduled for {}", peer.id());

    Ok(())
}
