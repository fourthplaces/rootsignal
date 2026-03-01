// Enrichment activities: actor extraction, location, quality, etc.
// Canonical location: crate::enrichment::*

pub use crate::enrichment::actor_extractor;
pub use crate::enrichment::actor_location;
pub use crate::enrichment::domain_filter;
pub use crate::enrichment::link_promoter;
pub use crate::enrichment::quality;
pub use crate::enrichment::universe_check;

use std::collections::HashSet;

use chrono::Utc;
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::SourceNode;
use rootsignal_graph::{GraphClient, GraphWriter};

use crate::core::events::{PipelineEvent, ScoutEvent};
use crate::infra::embedder::TextEmbedder;
use crate::traits::SignalReader;

/// Run post-scrape enrichment: delete consumed pins, actor extraction, embeddings, metrics.
/// Returns enrichment events.
pub async fn run_post_scrape(
    store: &dyn SignalReader,
    graph_client: &GraphClient,
    region: &rootsignal_common::ScoutScope,
    api_key: &str,
    embedder: &dyn TextEmbedder,
    consumed_pin_ids: &[Uuid],
) -> Vec<ScoutEvent> {
    let writer = GraphWriter::new(graph_client.clone());

    // Delete consumed pins
    if !consumed_pin_ids.is_empty() {
        match writer.delete_pins(consumed_pin_ids).await {
            Ok(()) => info!(count = consumed_pin_ids.len(), "Deleted consumed pins"),
            Err(e) => warn!(error = %e, "Failed to delete consumed pins"),
        }
    }

    // Actor extraction
    info!("=== Actor Extraction ===");
    let (min_lat, max_lat, min_lng, max_lng) = region.bounding_box();
    let (actor_stats, actor_events) =
        crate::enrichment::actor_extractor::run_actor_extraction(
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

    actor_events
}

/// Update source weights and cadence metrics.
pub async fn update_metrics(
    writer: &GraphWriter,
    region_name: &str,
    all_sources: &[SourceNode],
    source_signal_counts: &std::collections::HashMap<String, u32>,
    query_api_errors: &HashSet<String>,
) -> Vec<ScoutEvent> {
    let metrics = crate::scheduling::metrics::Metrics::new(writer, region_name);
    metrics
        .update(all_sources, source_signal_counts, query_api_errors, Utc::now())
        .await
}
