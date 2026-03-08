// Synthesis domain: similarity edges, response mapping, severity inference.

pub mod activities;
pub mod events;

#[cfg(test)]
mod completion_tests;

use anyhow::Result;
use futures::stream::{self, StreamExt};
use seesaw_core::{events, handle, handlers, Context, Events};
use tracing::{info, warn};

use rootsignal_common::events::SystemEvent;
use rootsignal_common::{Block, ChecklistItem};

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::domains::expansion::events::ExpansionEvent;
use crate::domains::scheduling::activities::budget::OperationCost;
use crate::domains::synthesis::events::SynthesisEvent;

fn is_expansion_completed(e: &ExpansionEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    matches!(e, ExpansionEvent::ExpansionCompleted { .. })
}

fn similarity_and_mapping_done(e: &SynthesisEvent, ctx: &Context<ScoutEngineDeps>) -> bool {
    if matches!(e, SynthesisEvent::SeverityInferred) { return false; }
    let (_, state) = ctx.singleton::<PipelineState>();
    state.similarity_computed && state.responses_mapped
}

fn describe_synthesis_progress(ctx: &Context<ScoutEngineDeps>) -> Vec<Block> {
    let (_, state) = ctx.singleton::<PipelineState>();
    vec![
        Block::Checklist {
            label: "Synthesis".into(),
            items: vec![
                ChecklistItem { text: "Similarity".into(), done: state.similarity_computed },
                ChecklistItem { text: "Response mapping".into(), done: state.responses_mapped },
            ],
        },
    ]
}

fn describe_synthesis_gate(ctx: &Context<ScoutEngineDeps>) -> Vec<Block> {
    let (_, state) = ctx.singleton::<PipelineState>();
    vec![
        Block::Checklist {
            label: "Synthesis".into(),
            items: vec![
                ChecklistItem { text: "Similarity".into(), done: state.similarity_computed },
                ChecklistItem { text: "Response mapping".into(), done: state.responses_mapped },
            ],
        },
    ]
}

#[handlers]
pub mod handlers {
    use super::*;

    // ---------------------------------------------------------------
    // Similarity: single graph-wide operation, not atomized
    // ---------------------------------------------------------------

    #[handle(on = ExpansionEvent, id = "synthesis:compute_similarity", filter = is_expansion_completed, describe = describe_synthesis_progress)]
    async fn compute_similarity(
        _event: ExpansionEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();

        let graph = match deps.graph.as_deref() {
            Some(g) => g,
            None => {
                return Ok(events![SynthesisEvent::SimilarityComputed]);
            }
        };

        info!("Building similarity edges...");
        match graph.compute_similarity_edges().await {
            Ok(edges) => {
                info!(edges = edges.len(), "Similarity edges computed");
                let mut out = Events::new();
                out.push(SystemEvent::SimilarityEdgesRebuilt { edges });
                out.push(SynthesisEvent::SimilarityComputed);
                Ok(out)
            }
            Err(e) => {
                warn!(error = %e, "Similarity edge building failed (non-fatal)");
                Ok(events![SynthesisEvent::SimilarityComputed])
            }
        }
    }

    // ===============================================================
    // ResponseMapping: guards deps, loads targets, processes all
    // ===============================================================

    #[handle(on = ExpansionEvent, id = "synthesis:map_responses", filter = is_expansion_completed, describe = describe_synthesis_progress)]
    async fn map_responses(
        _event: ExpansionEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let (_, state) = ctx.singleton::<PipelineState>();

        let (region, graph, budget) = match (
            state.run_scope.region(),
            deps.graph.as_deref(),
            deps.budget.as_ref(),
        ) {
            (Some(r), Some(g), Some(b)) => (r, g, b),
            _ => {
                return Ok(events![SynthesisEvent::ResponsesMapped]);
            }
        };

        let mut out = Events::new();
        if !budget.has_budget(OperationCost::CLAUDE_HAIKU_SYNTHESIS * 10) {
            ctx.logger.debug("Skipped response mapping: insufficient budget");
            out.push(SynthesisEvent::ResponsesMapped);
            return Ok(out);
        }

        let (min_lat, max_lat, min_lng, max_lng) = region.bounding_box();
        let tensions = match graph
            .get_active_tensions(min_lat, max_lat, min_lng, max_lng)
            .await
        {
            Ok(t) => t,
            Err(e) => {
                warn!(error = %e, "Failed to get active tensions for response mapping");
                out.push(SynthesisEvent::ResponsesMapped);
                return Ok(out);
            }
        };

        if tensions.is_empty() {
            info!("No active tensions for response mapping");
            out.push(SynthesisEvent::ResponsesMapped);
            return Ok(out);
        }

        info!(count = tensions.len(), "Processing response mapping targets");

        let ai = deps.ai.as_ref().expect("ai required for synthesis");

        let futures: Vec<_> = tensions
            .iter()
            .map(|(concern_id, embedding)| {
                activities::response_mapper::map_single_tension(
                    graph,
                    ai.as_ref(),
                    *concern_id,
                    embedding,
                    min_lat,
                    max_lat,
                    min_lng,
                    max_lng,
                )
            })
            .collect();
        let results: Vec<_> = stream::iter(futures).buffer_unordered(5).collect().await;

        for links in results {
            for link in links {
                out.push(SystemEvent::ResponseLinked {
                    signal_id: link.signal_id,
                    concern_id: link.concern_id,
                    strength: link.strength,
                    explanation: link.explanation,
                    source_url: None,
                });
            }
        }

        out.push(SynthesisEvent::ResponsesMapped);

        Ok(out)
    }

    // ---------------------------------------------------------------
    // Severity inference: triggers when all synthesis roles done
    // ---------------------------------------------------------------

    #[handle(on = SynthesisEvent, id = "synthesis:infer_severity", filter = similarity_and_mapping_done, describe = describe_synthesis_gate)]
    async fn infer_severity(
        _event: SynthesisEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let (_, state) = ctx.singleton::<PipelineState>();

        let (region, graph) = match (state.run_scope.region(), deps.graph.as_deref()) {
            (Some(r), Some(g)) => (r, g),
            _ => {
                ctx.logger.debug("Skipped severity inference: missing region or graph");
                return Ok(events![SynthesisEvent::SeverityInferred]);
            }
        };

        let (min_lat, max_lat, min_lng, max_lng) = region.bounding_box();

        let mut out = Events::new();
        match graph.compute_severity_inference(
            min_lat, max_lat, min_lng, max_lng,
        )
        .await
        {
            Ok(revisions) => {
                if !revisions.is_empty() {
                    info!(updated = revisions.len(), "Severity inference updated notices");
                }
                for rev in revisions {
                    out.push(SystemEvent::SeverityClassified {
                        signal_id: rev.signal_id,
                        severity: rev.severity,
                    });
                }
            }
            Err(e) => {
                warn!(error = %e, "Severity inference failed (non-fatal)");
                ctx.logger.debug(&format!("Severity inference failed: {e}"));
            }
        }
        out.push(SynthesisEvent::SeverityInferred);
        Ok(out)
    }
}
