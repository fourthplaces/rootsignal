//! News scanning activities — scan RSS feeds and extract signals.

use std::sync::Arc;

use tracing::{info, warn};

use rootsignal_graph::GraphStore;

use crate::core::engine::ScoutEngineDeps;

/// Scan news feeds for signals.
pub async fn scan_news(deps: &ScoutEngineDeps, events: &mut seesaw_core::Events) {
    let (archive, ai, gr, budget) = match (
        deps.archive.as_ref(),
        deps.ai.as_ref(),
        deps.graph.as_ref(),
        deps.budget.as_ref(),
    ) {
        (Some(a), Some(k), Some(g), Some(b)) => (a, k, g, b),
        _ => {
            tracing::debug!("News scan skipped: missing archive, ai, graph, or budget");
            return;
        }
    };

    let graph = GraphStore::new(gr.client().clone());
    let scanner = crate::news_scanner::NewsScanner::new(
        Arc::clone(archive),
        Arc::clone(ai),
        graph,
        budget.daily_limit(),
    );

    match scanner.scan().await {
        Ok(articles_scanned) => {
            info!(articles_scanned, "News scan complete");
        }
        Err(e) => warn!(error = %e, "News scan failed"),
    }
}
