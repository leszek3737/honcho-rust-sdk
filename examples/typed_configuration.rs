#![allow(clippy::print_stdout)]
//! Typed workspace configuration: get and set structured config.
//!
//! Demonstrates the typed `WorkspaceConfiguration` API (preferred) and, as a
//! contrast, the raw JSON escape hatch for fields the SDK does not yet model.
//!
//! Run with `cargo run --example typed_configuration`

use honcho_ai::Honcho;
use honcho_ai::types::common::{DreamConfiguration, ReasoningConfiguration, SummaryConfiguration};
use honcho_ai::types::workspace::WorkspaceConfiguration;

#[tokio::main]
async fn main() -> honcho_ai::error::Result<()> {
    let honcho = Honcho::new("http://localhost:8000", "config-demo")?;

    let config = honcho.get_configuration().await?;
    println!("Current config: {config:#?}");

    if let Some(reasoning) = &config.reasoning {
        println!("Reasoning enabled: {:?}", reasoning.enabled);
    }

    // Preferred path: build a typed `WorkspaceConfiguration` and write it via
    // `set_configuration`. The config structs are `#[non_exhaustive]` with no
    // builders, so they cannot be built with a struct literal (nor with
    // `..Default::default()`) from outside the crate — start from `Default`
    // and set the fields you need.
    let mut reasoning = ReasoningConfiguration::default();
    reasoning.enabled = Some(true);
    reasoning.custom_instructions = Some("Focus on user preferences".to_owned());

    let mut summary = SummaryConfiguration::default();
    summary.enabled = Some(true);
    summary.messages_per_short_summary = Some(20);
    summary.messages_per_long_summary = Some(60);

    let mut dream = DreamConfiguration::default();
    dream.enabled = Some(true);

    let mut new_config = WorkspaceConfiguration::default();
    new_config.reasoning = Some(reasoning);
    new_config.summary = Some(summary);
    new_config.dream = Some(dream);

    honcho.set_configuration(&new_config).await?;
    println!("Configuration updated via the typed API");

    let updated = honcho.get_configuration().await?;
    println!(
        "Reasoning custom instructions: {:?}",
        updated
            .reasoning
            .as_ref()
            .and_then(|r| r.custom_instructions.as_ref())
    );

    // Escape hatch: `set_configuration_raw` accepts arbitrary JSON for fields
    // the typed API does not model yet. Prefer `set_configuration` above.
    //
    // Warning: this is a PUT (full replace), not a merge — the map below
    // overwrites the *entire* configuration, dropping any field it omits
    // (e.g. `peer_card` or server fields the SDK does not represent). Read,
    // mutate, and write back the whole config if you must preserve them.
    let mut raw_config = honcho.get_configuration_raw().await?;
    raw_config.insert("dream".to_owned(), serde_json::json!({ "enabled": false }));
    honcho.set_configuration_raw(raw_config).await?;
    println!("Configuration updated via raw JSON (full PUT replace)");

    let raw = honcho.get_configuration_raw().await?;
    // `HashMap` iteration order is non-deterministic — sort for stable output.
    let mut keys: Vec<&String> = raw.keys().collect();
    keys.sort();
    println!("Raw config keys: {keys:?}");

    Ok(())
}
