// Enrichment activities: actor extraction, location, quality, diversity, etc.

pub mod actor_extractor;
pub mod actor_location;
pub mod actor_stats;
pub mod diversity;
pub mod domain_filter;
pub mod link_promoter;
pub mod quality;
pub mod universe_check;

use std::collections::{HashMap, HashSet};

use chrono::Utc;
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::events::SystemEvent;
use rootsignal_common::SourceNode;
use rootsignal_graph::{GraphClient, GraphReader};

use crate::domains::scheduling::activities::metrics::Metrics;
use crate::traits::SignalReader;

/// Compute post-scrape enrichment: emit PinsConsumed, actor extraction, diversity metrics, actor stats.
/// Returns enrichment events.
pub async fn compute_post_scrape_enrichment(
    store: &dyn SignalReader,
    graph_client: &GraphClient,
    region: &rootsignal_common::ScoutScope,
    api_key: &str,
    consumed_pin_ids: &[Uuid],
) -> seesaw_core::Events {
    let mut pin_events = seesaw_core::Events::new();

    // Emit PinsConsumed event (projector handles the graph delete)
    if !consumed_pin_ids.is_empty() {
        info!(count = consumed_pin_ids.len(), "Emitting PinsConsumed for consumed pins");
        pin_events.push(SystemEvent::PinsConsumed {
            pin_ids: consumed_pin_ids.to_vec(),
        });
    }

    // Actor extraction
    info!("=== Actor Extraction ===");
    let (min_lat, max_lat, min_lng, max_lng) = region.bounding_box();
    let (actor_stats, actor_events) =
        actor_extractor::run_actor_extraction(
            store,
            graph_client,
            api_key,
            min_lat,
            max_lat,
            min_lng,
            max_lng,
        )
        .await;
    info!("{actor_stats}");

    // Diversity metrics (event-sourced — projector handles the graph write)
    info!("=== Diversity Metrics ===");
    let reader = GraphReader::new(graph_client.clone());
    let diversity_events = diversity::compute_diversity_events(&reader, &[]).await;

    // Actor stats (event-sourced — projector handles the graph write)
    info!("=== Actor Stats ===");
    let actor_stats_events = actor_stats::compute_actor_stats_events(&reader).await;

    pin_events.extend(actor_events);
    pin_events.extend(diversity_events);
    pin_events.extend(actor_stats_events);
    pin_events
}

/// Compute source weight and cadence metrics, returning events.
pub async fn compute_source_metrics(
    graph: &GraphReader,
    region_name: &str,
    all_sources: &[SourceNode],
    source_signal_counts: &HashMap<String, u32>,
    query_api_errors: &HashSet<String>,
) -> seesaw_core::Events {
    let metrics = Metrics::new(graph, region_name);
    metrics
        .compute_source_metrics(all_sources, source_signal_counts, query_api_errors, Utc::now())
        .await
}
