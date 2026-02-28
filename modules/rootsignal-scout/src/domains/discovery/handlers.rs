//! Seesaw handlers for the discovery domain.

use std::sync::Arc;

use seesaw_core::{handler::Emit, on, Context, Handler};

use crate::core::engine::ScoutEngineDeps;
use crate::core::events::{PipelineEvent, PipelinePhase, ScoutEvent};
use crate::enrichment::link_promoter::{self, PromotionConfig};
use crate::pipeline::handlers::bootstrap;

fn batch(events: Vec<ScoutEvent>) -> Emit<ScoutEvent> {
    Emit::Batch(events)
}

/// EngineStarted → seed sources when the region has none.
pub fn bootstrap_handler() -> Handler<ScoutEngineDeps> {
    on::<ScoutEvent>()
        .id("discovery:bootstrap")
        .filter(|e: &ScoutEvent| {
            matches!(
                e,
                ScoutEvent::Pipeline(PipelineEvent::EngineStarted { .. })
            )
        })
        .then::<ScoutEngineDeps, _, _, _, _, ScoutEvent>(
            |event: Arc<ScoutEvent>, ctx: Context<ScoutEngineDeps>| async move {
                let deps = ctx.deps();
                let state = deps.state.read().await;
                let pipe = deps.pipeline_deps.read().await;
                let pipe = pipe.as_ref().expect("pipeline_deps set by dispatch");
                let events =
                    bootstrap::handle_engine_started(&state, pipe).await?;
                Ok(batch(events))
            },
        )
}

/// PhaseCompleted(TensionScrape|ResponseScrape|Expansion) → promote collected links to sources.
pub fn link_promotion_handler() -> Handler<ScoutEngineDeps> {
    on::<ScoutEvent>()
        .id("discovery:link_promotion")
        .filter(|e: &ScoutEvent| {
            matches!(
                e,
                ScoutEvent::Pipeline(PipelineEvent::PhaseCompleted { phase })
                    if matches!(phase, PipelinePhase::TensionScrape | PipelinePhase::ResponseScrape | PipelinePhase::Expansion)
            )
        })
        .then::<ScoutEngineDeps, _, _, _, _, ScoutEvent>(
            |_event: Arc<ScoutEvent>, ctx: Context<ScoutEngineDeps>| async move {
                let deps = ctx.deps();
                let state = deps.state.read().await;
                if state.collected_links.is_empty() {
                    return Ok(Emit::None);
                }
                let links = state.collected_links.clone();
                drop(state);

                let promoted = link_promoter::promote_links(&links, &PromotionConfig::default());
                if promoted.is_empty() {
                    return Ok(Emit::None);
                }
                let count = promoted.len() as u32;
                let mut events: Vec<ScoutEvent> = promoted
                    .into_iter()
                    .map(|s| {
                        ScoutEvent::Pipeline(PipelineEvent::SourceDiscovered {
                            source: s,
                            discovered_by: "link_promoter".into(),
                        })
                    })
                    .collect();
                events.push(ScoutEvent::Pipeline(PipelineEvent::LinksPromoted { count }));
                Ok(batch(events))
            },
        )
}
