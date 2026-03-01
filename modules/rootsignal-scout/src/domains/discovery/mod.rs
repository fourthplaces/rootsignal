// Discovery domain: finding sources, responses, tensions.

pub mod activities;
pub mod events;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};
use tracing::info;

use rootsignal_graph::GraphReader;

use crate::core::engine::ScoutEngineDeps;
use crate::core::events::PipelinePhase;
use crate::domains::discovery::events::DiscoveryEvent;
use crate::domains::enrichment::activities::link_promoter::{self, PromotionConfig};
use crate::domains::discovery::activities::{bootstrap, discover_sources_mid_run};
use crate::domains::lifecycle::events::LifecycleEvent;

fn is_engine_started(e: &LifecycleEvent) -> bool {
    matches!(e, LifecycleEvent::EngineStarted { .. })
}

fn is_scrape_or_expansion_completed(e: &LifecycleEvent) -> bool {
    matches!(
        e,
        LifecycleEvent::PhaseCompleted { phase }
            if matches!(phase, PipelinePhase::TensionScrape | PipelinePhase::ResponseScrape | PipelinePhase::Expansion)
    )
}

fn is_tension_scrape_completed(e: &LifecycleEvent) -> bool {
    matches!(
        e,
        LifecycleEvent::PhaseCompleted { phase }
            if matches!(phase, PipelinePhase::TensionScrape)
    )
}

#[handlers]
pub mod handlers {
    use super::*;

    /// EngineStarted → seed sources when the region has none.
    #[handle(on = LifecycleEvent, id = "discovery:bootstrap", filter = is_engine_started)]
    async fn bootstrap(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let state = deps.state.read().await;
        let events = bootstrap::seed_sources_if_empty(&state, deps).await?;
        Ok(events)
    }

    /// PhaseCompleted(TensionScrape|ResponseScrape|Expansion) → promote collected links to sources.
    #[handle(on = LifecycleEvent, id = "discovery:link_promotion", filter = is_scrape_or_expansion_completed)]
    async fn link_promotion(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
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
        let mut events = Events::new();
        for s in promoted {
            events.push(DiscoveryEvent::SourceDiscovered {
                source: s,
                discovered_by: "link_promoter".into(),
            });
        }
        events.push(DiscoveryEvent::LinksPromoted { count });
        Ok(events)
    }

    /// PhaseCompleted(TensionScrape) → discover mid-run sources, emit PhaseCompleted(MidRunDiscovery).
    #[handle(on = LifecycleEvent, id = "discovery:mid_run", filter = is_tension_scrape_completed)]
    async fn mid_run_discovery(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
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
        let graph = GraphReader::new(graph_client.clone());

        let output = discover_sources_mid_run(
            &graph,
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

        let mut all_events = output.events;
        all_events.push(LifecycleEvent::PhaseCompleted {
            phase: PipelinePhase::MidRunDiscovery,
        });
        Ok(all_events)
    }
}
