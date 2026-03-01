//! Seesaw handlers for the discovery domain.
//!
//! Thin wrappers that delegate to activity functions.

use std::sync::Arc;

use seesaw_core::{events, on, Context, Events, Handler};
use tracing::info;

use rootsignal_graph::GraphWriter;

use crate::core::engine::ScoutEngineDeps;
use crate::core::events::{PipelinePhase, ScoutEvent};
use crate::domains::lifecycle::events::LifecycleEvent;
use crate::enrichment::link_promoter::{self, PromotionConfig};
use crate::domains::discovery::activities::bootstrap;

/// EngineStarted → seed sources when the region has none.
pub fn bootstrap_handler() -> Handler<ScoutEngineDeps> {
    on::<LifecycleEvent>()
        .id("discovery:bootstrap")
        .filter(|e: &LifecycleEvent| {
            matches!(e, LifecycleEvent::EngineStarted { .. })
        })
        .then(
            |_event: Arc<LifecycleEvent>, ctx: Context<ScoutEngineDeps>| async move {
                let deps = ctx.deps();
                let state = deps.state.read().await;
                let events =
                    bootstrap::handle_engine_started(&state, deps).await?;
                Ok(events![..events])
            },
        )
}

/// PhaseCompleted(TensionScrape|ResponseScrape|Expansion) → promote collected links to sources.
pub fn link_promotion_handler() -> Handler<ScoutEngineDeps> {
    on::<LifecycleEvent>()
        .id("discovery:link_promotion")
        .filter(|e: &LifecycleEvent| {
            matches!(
                e,
                LifecycleEvent::PhaseCompleted { phase }
                    if matches!(phase, PipelinePhase::TensionScrape | PipelinePhase::ResponseScrape | PipelinePhase::Expansion)
            )
        })
        .then(
            |_event: Arc<LifecycleEvent>, ctx: Context<ScoutEngineDeps>| async move {
                let deps = ctx.deps();
                let state = deps.state.read().await;
                if state.collected_links.is_empty() {
                    return Ok(events![]);
                }
                let links = state.collected_links.clone();
                drop(state);

                let promoted = link_promoter::promote_links(&links, &PromotionConfig::default());
                if promoted.is_empty() {
                    return Ok(events![]);
                }
                let count = promoted.len() as u32;
                let mut events: Vec<ScoutEvent> = promoted
                    .into_iter()
                    .map(|s| {
                        ScoutEvent::Pipeline(crate::core::events::PipelineEvent::SourceDiscovered {
                            source: s,
                            discovered_by: "link_promoter".into(),
                        })
                    })
                    .collect();
                events.push(ScoutEvent::Pipeline(crate::core::events::PipelineEvent::LinksPromoted { count }));
                Ok(events![..events])
            },
        )
}

/// PhaseCompleted(TensionScrape) → discover mid-run sources, emit PhaseCompleted(MidRunDiscovery).
pub fn mid_run_handler() -> Handler<ScoutEngineDeps> {
    on::<LifecycleEvent>()
        .id("discovery:mid_run")
        .filter(|e: &LifecycleEvent| {
            matches!(
                e,
                LifecycleEvent::PhaseCompleted { phase }
                    if matches!(phase, PipelinePhase::TensionScrape)
            )
        })
        .then(
            |_event: Arc<LifecycleEvent>, ctx: Context<ScoutEngineDeps>| async move {
                info!("=== Mid-Run Discovery ===");
                let deps = ctx.deps();

                // Requires graph_client + budget — skip in tests
                let (region, graph_client, budget) = match (
                    deps.region.as_ref(),
                    deps.graph_client.as_ref(),
                    deps.budget.as_ref(),
                ) {
                    (Some(r), Some(g), Some(b)) => (r, g, b),
                    _ => {
                        return Ok(events![LifecycleEvent::PhaseCompleted {
                            phase: PipelinePhase::MidRunDiscovery,
                        }]);
                    }
                };
                let writer = GraphWriter::new(graph_client.clone());

                let output = crate::domains::discovery::activities::discover_mid_run(
                    &writer,
                    &region.name,
                    &*deps.embedder,
                    deps.anthropic_api_key.as_deref(),
                    budget,
                )
                .await;

                // Stash social topics for response scrape
                if !output.social_topics.is_empty() {
                    let mut state = deps.state.write().await;
                    state.social_topics = output.social_topics;
                }

                Ok(Events::batch(output.events).add(LifecycleEvent::PhaseCompleted {
                    phase: PipelinePhase::MidRunDiscovery,
                }))
            },
        )
}
