//! Bootstrap helpers — tension-seeded follow-up queries.
//!
//! The cold-start bootstrap logic now lives in `pipeline::handlers::bootstrap`
//! and is triggered by `EngineStarted`. This module retains the tension-seed
//! query generator which runs after the first scrape phase.

use anyhow::Result;
use tracing::info;

use rootsignal_common::{canonical_value, DiscoveryMethod, ScoutScope, SourceNode, SourceRole};
use rootsignal_graph::GraphWriter;

/// Generate tension-seeded follow-up queries from existing tensions.
/// For each tension, creates targeted search queries to find organizations helping.
pub async fn tension_seed_queries(
    writer: &GraphWriter,
    region: &ScoutScope,
) -> Result<Vec<SourceNode>> {
    // Get existing tensions from the graph
    let tensions = writer.get_recent_tensions(10).await.unwrap_or_default();
    if tensions.is_empty() {
        info!("No tensions found for tension-seeded discovery");
        return Ok(Vec::new());
    }

    let region_name = &region.name;
    let mut all_sources = Vec::new();

    for (title, what_would_help) in &tensions {
        let help_text = what_would_help.as_deref().unwrap_or(title);
        let query = format!(
            "organizations helping with {} in {}",
            help_text, region_name
        );

        let cv = query.clone();
        let ck = canonical_value(&cv);
        all_sources.push(SourceNode::new(
            ck,
            cv,
            None,
            DiscoveryMethod::TensionSeed,
            0.5,
            SourceRole::Response,
            Some(format!("Tension: {title}")),
        ));
    }

    info!(
        queries = all_sources.len(),
        "Generated tension-seeded queries"
    );
    Ok(all_sources)
}
