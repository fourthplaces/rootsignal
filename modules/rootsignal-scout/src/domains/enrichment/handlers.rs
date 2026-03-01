//! Seesaw handlers for the enrichment domain.
//!
//! Thin wrappers that delegate to activity functions.

use std::sync::Arc;

use seesaw_core::{events, on, Context, Handler};
use tracing::info;

use rootsignal_graph::GraphWriter;

use crate::core::engine::ScoutEngineDeps;
use crate::core::events::{PipelineEvent, PipelinePhase, ScoutEvent};
use crate::domains::enrichment::activities;
use crate::domains::lifecycle::events::LifecycleEvent;

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
                    activities::actor_location::collect_actor_location_events(&*deps.store, &actors).await;
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

                let consumed_pin_ids = {
                    let state = deps.state.read().await;
                    state
                        .scheduled
                        .as_ref()
                        .map(|s| s.consumed_pin_ids.clone())
                        .unwrap_or_default()
                };

                let actor_events = activities::run_post_scrape(
                    &*deps.store,
                    graph_client,
                    region,
                    deps.anthropic_api_key.as_deref().unwrap_or(""),
                    &*deps.embedder,
                    &consumed_pin_ids,
                )
                .await;

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

                let metric_events = activities::update_metrics(
                    &writer,
                    &region.name,
                    &all_sources,
                    &source_signal_counts,
                    &query_api_errors,
                )
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
