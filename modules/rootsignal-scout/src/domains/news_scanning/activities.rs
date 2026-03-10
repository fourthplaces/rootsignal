//! News scanning activities — scan RSS feeds and extract signals.

use std::sync::Arc;

use tracing::{info, warn};

use rootsignal_graph::GraphStore;

use crate::core::engine::ScoutEngineDeps;

/// Scan news feeds for signals.
pub async fn scan_news(deps: &ScoutEngineDeps, daily_budget_cents: u64, events: &mut seesaw_core::Events) {
    let (archive, ai) = match (
        deps.archive.as_ref(),
        deps.ai.as_ref(),
    ) {
        (Some(a), Some(k)) => (a, k),
        _ => {
            tracing::debug!("News scan skipped: missing archive or ai");
            return;
        }
    };

    let graph_client = match deps.graph_client.as_ref() {
        Some(c) => c,
        None => {
            tracing::debug!("News scan skipped: missing graph_client");
            return;
        }
    };
    let graph = GraphStore::new(graph_client.clone());
    let scanner = crate::news_scanner::NewsScanner::new(
        Arc::clone(archive),
        Arc::clone(ai),
        graph,
        daily_budget_cents,
    );

    match scanner.scan().await {
        Ok(articles_scanned) => {
            info!(articles_scanned, "News scan complete");
        }
        Err(e) => warn!(error = %e, "News scan failed"),
    }
}
