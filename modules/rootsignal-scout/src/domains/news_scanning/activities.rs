//! News scanning activities — scan RSS feeds and extract signals.

use std::sync::Arc;

use tracing::{info, warn};

use rootsignal_common::telemetry_events::TelemetryEvent;
use rootsignal_graph::GraphStore;

use crate::core::engine::ScoutEngineDeps;

/// Scan news feeds for signals.
pub async fn scan_news(deps: &ScoutEngineDeps, events: &mut seesaw_core::Events) {
    let (archive, ai, graph_client, budget) = match (
        deps.archive.as_ref(),
        deps.ai.as_ref(),
        deps.graph_client.as_ref(),
        deps.budget.as_ref(),
    ) {
        (Some(a), Some(k), Some(g), Some(b)) => (a, k, g, b),
        _ => {
            warn!("News scan skipped: missing archive, ai, graph_client, or budget");
            events.push(TelemetryEvent::SystemLog {
                message: "Skipped news scan: missing archive, ai, graph_client, or budget".into(),
                context: Some(serde_json::json!({
                    "handler": "news_scanning:scan_news",
                    "reason": "missing_deps",
                    "missing": {
                        "archive": deps.archive.is_none(),
                        "ai": deps.ai.is_none(),
                        "graph_client": deps.graph_client.is_none(),
                        "budget": deps.budget.is_none(),
                    },
                })),
            });
            return;
        }
    };

    let graph = GraphStore::new(graph_client.clone());
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
