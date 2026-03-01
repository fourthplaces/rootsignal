//! Seesaw handlers for the synthesis domain.
//!
//! Thin wrapper that delegates to the `run_synthesis` activity.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use seesaw_core::{events, on, Context, Handler};

use rootsignal_graph::GraphWriter;

use crate::core::engine::ScoutEngineDeps;
use crate::core::events::PipelinePhase;
use crate::domains::lifecycle::events::LifecycleEvent;
use crate::domains::synthesis::activities;

/// PhaseCompleted(Expansion) → similarity edges, parallel finders, severity inference,
/// emit PhaseCompleted(Synthesis).
pub fn synthesis_handler() -> Handler<ScoutEngineDeps> {
    on::<LifecycleEvent>()
        .id("synthesis:run")
        .filter(|e: &LifecycleEvent| {
            matches!(
                e,
                LifecycleEvent::PhaseCompleted { phase }
                    if matches!(phase, PipelinePhase::Expansion)
            )
        })
        .then(
            |_event: Arc<LifecycleEvent>, ctx: Context<ScoutEngineDeps>| async move {
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

                let writer = GraphWriter::new(graph_client.clone());
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
                    &writer,
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
            },
        )
}
