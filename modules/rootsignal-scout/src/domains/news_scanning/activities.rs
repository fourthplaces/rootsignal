//! News scanning activities — scan RSS feeds and emit BeaconDetected events.

use std::sync::Arc;

use tracing::{info, warn};

use rootsignal_common::events::SystemEvent;
use rootsignal_graph::GraphStore;

use crate::core::engine::ScoutEngineDeps;

/// Scan news feeds and push BeaconDetected events for each new beacon task.
pub async fn scan_news(deps: &ScoutEngineDeps, events: &mut seesaw_core::Events) {
    let (archive, api_key, graph_client, budget) = match (
        deps.archive.as_ref(),
        deps.anthropic_api_key.as_deref(),
        deps.graph_client.as_ref(),
        deps.budget.as_ref(),
    ) {
        (Some(a), Some(k), Some(g), Some(b)) => (a, k, g, b),
        _ => {
            warn!("News scan skipped: missing archive, api_key, graph_client, or budget");
            return;
        }
    };

    let graph = GraphStore::new(graph_client.clone());
    let scanner = crate::news_scanner::NewsScanner::new(
        Arc::clone(archive),
        api_key,
        graph,
        budget.daily_limit(),
    );

    match scanner.scan().await {
        Ok((articles_scanned, beacon_tasks)) => {
            info!(articles_scanned, beacons = beacon_tasks.len(), "News scan complete");
            for task in beacon_tasks {
                events.push(SystemEvent::BeaconDetected { task });
            }
        }
        Err(e) => warn!(error = %e, "News scan failed"),
    }
}
