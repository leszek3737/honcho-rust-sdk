//! Integration test harness for `honcho-ai`.
//!
//! These tests exercise the SDK against a **running Honcho server** and are
//! gated behind the `integration` Cargo feature (see the `[[test]]` entry in
//! `Cargo.toml`). Without `--features integration` the target is not built, so
//! a green `cargo test` means **zero integration tests ran** — not that they
//! passed.
//!
//! Even with the feature enabled, [`common::try_client`] soft-skips (returns
//! `None`, logging the cause) when no server is reachable, so coverage depends
//! on a server actually being configured.
//!
//! Every test that constructs a [`common::WorkspaceGuard`] MUST be annotated
//! `#[tokio::test(flavor = "multi_thread")]`: the guard's `Drop` calls
//! `tokio::task::block_in_place`, which panics on a current-thread runtime.
//!
//! The crate-level lint allows below are the single source for the whole
//! integration target; submodules should not duplicate them.
#![allow(clippy::print_stderr)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
mod lifecycle;
mod streaming;
mod upload;
