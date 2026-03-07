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
use crate::domains::synthesis::events::{
    all_synthesis_roles, SynthesisEvent, SynthesisRole,
};

fn is_expansion_completed(e: &ExpansionEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    matches!(e, ExpansionEvent::ExpansionCompleted { .. })
}

fn all_synthesis_done(e: &SynthesisEvent, ctx: &Context<ScoutEngineDeps>) -> bool {
    let role = match e {
        SynthesisEvent::SynthesisRoleCompleted { role, .. } => role,
        _ => return false,
    };
    let (_, state) = ctx.singleton::<PipelineState>();
    state.synthesis_completing_role.as_ref() == Some(role)
}

fn describe_synthesis_progress(ctx: &Context<ScoutEngineDeps>) -> Vec<Block> {
    let (_, state) = ctx.singleton::<PipelineState>();
    let all = all_synthesis_roles();
    let done = &state.completed_synthesis_roles;
    let completed = done.len() as u32;
    let total = all.len() as u32;
    vec![
        Block::Checklist {
            label: "Synthesis roles".into(),
            items: all.iter().map(|r| ChecklistItem {
                text: format!("{r:?}"),
                done: done.contains(r),
            }).collect(),
        },
        Block::Progress {
            label: "Overall".into(),
            fraction: if total > 0 { completed as f32 / total as f32 } else { 0.0 },
        },
    ]
}

fn describe_synthesis_gate(ctx: &Context<ScoutEngineDeps>) -> Vec<Block> {
    let (_, state) = ctx.singleton::<PipelineState>();
    let all = all_synthesis_roles();
    let done = &state.completed_synthesis_roles;
    vec![
        Block::Checklist {
            label: "Synthesis roles".into(),
            items: all.iter().map(|r| ChecklistItem {
                text: format!("{r:?}"),
                done: done.contains(r),
            }).collect(),
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
        let run_id = deps.run_id;

        let graph = match deps.graph.as_ref() {
            Some(g) => g,
            None => {
                return Ok(events![SynthesisEvent::SynthesisRoleCompleted {
                    run_id,
                    role: SynthesisRole::Similarity,
                }]);
            }
        };

        info!("Building similarity edges...");
        match rootsignal_graph::similarity::compute_edges(graph.client()).await {
            Ok(edges) => {
                info!(edges = edges.len(), "Similarity edges computed");
                let mut out = Events::new();
                out.push(SystemEvent::SimilarityEdgesRebuilt { edges });
                out.push(SynthesisEvent::SynthesisRoleCompleted {
                    run_id,
                    role: SynthesisRole::Similarity,
                });
                Ok(out)
            }
            Err(e) => {
                warn!(error = %e, "Similarity edge building failed (non-fatal)");
                Ok(events![SynthesisEvent::SynthesisRoleCompleted {
                    run_id,
                    role: SynthesisRole::Similarity,
                }])
            }
        }
    }

    // ===============================================================
    // ResponseMapping: guards deps, loads targets, processes all, emits SynthesisRoleCompleted
    // ===============================================================

    #[handle(on = ExpansionEvent, id = "synthesis:map_responses", filter = is_expansion_completed, describe = describe_synthesis_progress)]
    async fn map_responses(
        _event: ExpansionEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let (_, state) = ctx.singleton::<PipelineState>();
        let run_id = deps.run_id;

        let (region, graph, budget) = match (
            state.run_scope.region(),
            deps.graph.as_ref(),
            deps.budget.as_ref(),
        ) {
            (Some(r), Some(g), Some(b)) => (r, g, b),
            _ => {
                return Ok(events![SynthesisEvent::SynthesisRoleCompleted {
                    run_id,
                    role: SynthesisRole::ResponseMapping,
                }]);
            }
        };

        let mut out = Events::new();
        if !budget.has_budget(OperationCost::CLAUDE_HAIKU_SYNTHESIS * 10) {
            ctx.logger.debug("Skipped response mapping: insufficient budget");
            out.push(SynthesisEvent::SynthesisRoleCompleted {
                run_id,
                role: SynthesisRole::ResponseMapping,
            });
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
                out.push(SynthesisEvent::SynthesisRoleCompleted {
                    run_id,
                    role: SynthesisRole::ResponseMapping,
                });
                return Ok(out);
            }
        };

        if tensions.is_empty() {
            info!("No active tensions for response mapping");
            out.push(SynthesisEvent::SynthesisRoleCompleted {
                run_id,
                role: SynthesisRole::ResponseMapping,
            });
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

        for (target_events, _edges_created) in results {
            out.extend(target_events);
        }

        out.push(SynthesisEvent::SynthesisRoleCompleted {
            run_id,
            role: SynthesisRole::ResponseMapping,
        });

        Ok(out)
    }

    // ---------------------------------------------------------------
    // Severity inference: triggers when all synthesis roles done
    // ---------------------------------------------------------------

    #[handle(on = SynthesisEvent, id = "synthesis:infer_severity", filter = all_synthesis_done, describe = describe_synthesis_gate)]
    async fn infer_severity(
        _event: SynthesisEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let (_, state) = ctx.singleton::<PipelineState>();
        let run_id = deps.run_id;

        let (region, graph) = match (state.run_scope.region(), deps.graph.as_ref()) {
            (Some(r), Some(g)) => (r, g),
            _ => {
                ctx.logger.debug("Skipped severity inference: missing region or graph");
                return Ok(events![SynthesisEvent::SynthesisCompleted { run_id }]);
            }
        };

        let (min_lat, max_lat, min_lng, max_lng) = region.bounding_box();

        let mut all_events = match rootsignal_graph::severity_inference::compute_severity_inference(
            graph, min_lat, max_lat, min_lng, max_lng,
        )
        .await
        {
            Ok((updated, severity_events)) => {
                if updated > 0 {
                    info!(updated, "Severity inference updated notices");
                }
                let mut evts = Events::new();
                for ev in severity_events {
                    evts.push(ev);
                }
                evts
            }
            Err(e) => {
                warn!(error = %e, "Severity inference failed (non-fatal)");
                ctx.logger.debug(&format!("Severity inference failed: {e}"));
                Events::new()
            }
        };
        all_events.push(SynthesisEvent::SynthesisCompleted { run_id });
        Ok(all_events)
    }
}
