// Enrichment activities: actor extraction, location, quality, etc.

pub mod actor_extractor;
pub mod actor_location;
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
use crate::infra::embedder::TextEmbedder;
use crate::traits::SignalReader;

/// Compute post-scrape enrichment: emit PinsConsumed, actor extraction, embeddings, metrics.
/// Returns enrichment events.
pub async fn compute_post_scrape_enrichment(
    store: &dyn SignalReader,
    graph_client: &GraphClient,
    region: &rootsignal_common::ScoutScope,
    api_key: &str,
    embedder: &dyn TextEmbedder,
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

    // Embedding enrichment
    info!("=== Embedding Enrichment ===");
    match rootsignal_graph::enrich_embeddings(graph_client, embedder, 50).await {
        Ok(stats) => info!("{stats}"),
        Err(e) => warn!(error = %e, "Embedding enrichment failed, continuing"),
    }

    // Metric enrichment
    info!("=== Metric Enrichment ===");
    match rootsignal_graph::enrich(
        graph_client,
        &[],
        0.3,
        min_lat,
        max_lat,
        min_lng,
        max_lng,
    )
    .await
    {
        Ok(stats) => info!(?stats, "Metric enrichment complete"),
        Err(e) => warn!(error = %e, "Metric enrichment failed, continuing"),
    }

    pin_events.extend(actor_events);
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
