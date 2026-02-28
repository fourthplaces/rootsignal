//! Seesaw handlers for the enrichment domain.

use std::sync::Arc;

use chrono::Utc;
use seesaw_core::{events, on, Context, Handler};
use tracing::{info, warn};

use rootsignal_graph::GraphWriter;

use crate::core::engine::ScoutEngineDeps;
use crate::core::events::{PipelineEvent, PipelinePhase, ScoutEvent};
use crate::domains::lifecycle::events::LifecycleEvent;
use crate::enrichment::actor_location;

/// PhaseCompleted(ResponseScrape) → enrich actor locations from signal evidence.
pub fn actor_location_handler() -> Handler<ScoutEngineDeps> {
    on::<LifecycleEvent>()
        .id("enrichment:actor_location")
        .filter(|e: &LifecycleEvent| {
            matches!(
                e,
                LifecycleEvent::PhaseCompleted { phase }
                    if matches!(phase, PipelinePhase::ResponseScrape)
            )
        })
        .then(
            |_event: Arc<LifecycleEvent>, ctx: Context<ScoutEngineDeps>| async move {
                let deps = ctx.deps();

                let actors = match deps.store.list_all_actors().await {
                    Ok(a) => a,
                    Err(_) => return Ok(events![]),
                };
                if actors.is_empty() {
                    return Ok(events![]);
                }

                let events =
                    actor_location::collect_actor_location_events(&*deps.store, &actors).await;
                if events.is_empty() {
                    return Ok(events![]);
                }
                let actors_updated = events.len() as u32;
                let mut all_events = events;
                all_events.push(ScoutEvent::Pipeline(
                    PipelineEvent::ActorEnrichmentCompleted { actors_updated },
                ));
                Ok(events![..all_events])
            },
        )
}

/// PhaseCompleted(ResponseScrape) → delete pins, actor extraction, embedding + metric enrichment,
/// emit PhaseCompleted(ActorEnrichment).
pub fn post_scrape_handler() -> Handler<ScoutEngineDeps> {
    on::<LifecycleEvent>()
        .id("enrichment:post_scrape")
        .filter(|e: &LifecycleEvent| {
            matches!(
                e,
                LifecycleEvent::PhaseCompleted { phase }
                    if matches!(phase, PipelinePhase::ResponseScrape)
            )
        })
        .then(
            |_event: Arc<LifecycleEvent>, ctx: Context<ScoutEngineDeps>| async move {
                let deps = ctx.deps();

                // Requires graph_client + region — skip in tests
                let (region, graph_client) = match (deps.region.as_ref(), deps.graph_client.as_ref())
                {
                    (Some(r), Some(g)) => (r, g),
                    _ => {
                        return Ok(events![LifecycleEvent::PhaseCompleted {
                            phase: PipelinePhase::ActorEnrichment,
                        }]);
                    }
                };

                // Delete consumed pins
                {
                    let state = deps.state.read().await;
                    if let Some(ref scheduled) = state.scheduled {
                        if !scheduled.consumed_pin_ids.is_empty() {
                            let writer = GraphWriter::new(graph_client.clone());
                            match writer.delete_pins(&scheduled.consumed_pin_ids).await {
                                Ok(()) => info!(
                                    count = scheduled.consumed_pin_ids.len(),
                                    "Deleted consumed pins"
                                ),
                                Err(e) => warn!(error = %e, "Failed to delete consumed pins"),
                            }
                        }
                    }
                }

                // Actor extraction
                info!("=== Actor Extraction ===");
                let (min_lat, max_lat, min_lng, max_lng) = region.bounding_box();
                let (actor_stats, actor_events) =
                    crate::enrichment::actor_extractor::run_actor_extraction(
                        &*deps.store,
                        graph_client,
                        deps.anthropic_api_key.as_deref().unwrap_or(""),
                        min_lat,
                        max_lat,
                        min_lng,
                        max_lng,
                    )
                    .await;
                info!("{actor_stats}");

                // Embedding enrichment
                info!("=== Embedding Enrichment ===");
                match rootsignal_graph::enrich_embeddings(graph_client, &*deps.embedder, 50).await {
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

                let mut all_events = actor_events;
                all_events.push(ScoutEvent::Lifecycle(
                    LifecycleEvent::PhaseCompleted {
                        phase: PipelinePhase::ActorEnrichment,
                    },
                ));
                Ok(events![..all_events])
            },
        )
}

/// PhaseCompleted(ActorEnrichment) → update source weights/cadence, emit MetricsCompleted.
pub fn metrics_handler() -> Handler<ScoutEngineDeps> {
    on::<LifecycleEvent>()
        .id("enrichment:metrics")
        .filter(|e: &LifecycleEvent| {
            matches!(
                e,
                LifecycleEvent::PhaseCompleted { phase }
                    if matches!(phase, PipelinePhase::ActorEnrichment)
            )
        })
        .then(
            |_event: Arc<LifecycleEvent>, ctx: Context<ScoutEngineDeps>| async move {
                let deps = ctx.deps();

                // Requires graph_client + region — skip in tests
                let (region, graph_client) = match (deps.region.as_ref(), deps.graph_client.as_ref())
                {
                    (Some(r), Some(g)) => (r, g),
                    _ => {
                        return Ok(events![LifecycleEvent::MetricsCompleted]);
                    }
                };
                let writer = GraphWriter::new(graph_client.clone());

                let state = deps.state.read().await;
                let all_sources = state
                    .scheduled
                    .as_ref()
                    .map(|s| s.all_sources.clone())
                    .unwrap_or_default();
                let source_signal_counts = state.source_signal_counts.clone();
                let query_api_errors = state.query_api_errors.clone();
                drop(state);

                let metrics = crate::scheduling::metrics::Metrics::new(
                    &writer,
                    &region.name,
                );
                let metric_events = metrics
                    .update(&all_sources, &source_signal_counts, &query_api_errors, Utc::now())
                    .await;

                if let Some(ref budget) = deps.budget {
                    budget.log_status();
                }

                let mut all_events = metric_events;
                all_events.push(ScoutEvent::Lifecycle(LifecycleEvent::MetricsCompleted));
                Ok(events![..all_events])
            },
        )
}
