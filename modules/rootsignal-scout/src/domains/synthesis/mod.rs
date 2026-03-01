// Synthesis domain: similarity edges, parallel finders, severity inference.

pub mod activities;

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};

use rootsignal_graph::GraphStore;

use crate::core::engine::ScoutEngineDeps;
use crate::core::events::PipelinePhase;
use crate::domains::lifecycle::events::LifecycleEvent;

fn is_expansion_completed(e: &LifecycleEvent) -> bool {
    matches!(
        e,
        LifecycleEvent::PhaseCompleted { phase }
            if matches!(phase, PipelinePhase::Expansion)
    )
}

#[handlers]
pub mod handlers {
    use super::*;

    /// PhaseCompleted(Expansion) → similarity edges, parallel finders, severity inference,
    /// emit PhaseCompleted(Synthesis).
    #[handle(on = LifecycleEvent, id = "synthesis:run", filter = is_expansion_completed)]
    async fn synthesis(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();

        let (region, graph_client, budget) = match (
            deps.region.as_ref(),
            deps.graph_client.as_ref(),
            deps.budget.as_ref(),
        ) {
            (Some(r), Some(g), Some(b)) => (r, g, b),
            _ => {
                return Ok(events![LifecycleEvent::PhaseCompleted {
                    phase: PipelinePhase::Synthesis,
                }]);
            }
        };

        let graph = GraphStore::new(graph_client.clone());
        let cancelled = deps
            .cancelled
            .clone()
            .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
        let api_key = deps
            .anthropic_api_key
            .as_deref()
            .unwrap_or_default()
            .to_string();

        let archive = match deps.archive.as_ref() {
            Some(a) => a.clone(),
            None => {
                return Ok(events![LifecycleEvent::PhaseCompleted {
                    phase: PipelinePhase::Synthesis,
                }]);
            }
        };

        let output = activities::run_synthesis(
            &graph,
            graph_client,
            archive,
            &*deps.embedder,
            &api_key,
            region,
            budget,
            cancelled,
            deps.run_id.clone(),
        )
        .await;

        let mut all_events = output.events;
        all_events.push(LifecycleEvent::PhaseCompleted {
            phase: PipelinePhase::Synthesis,
        });
        Ok(all_events)
    }
}
