//! Situation weaving activities — pure functions + event-emitting orchestrator.

pub mod discover;
pub mod pure;
pub mod types;
pub mod weave;

use tracing::{info, warn};

use rootsignal_common::events::SystemEvent;
use crate::core::engine::ScoutEngineDeps;
use crate::domains::scheduling::activities::budget::OperationCost;

/// Run situation weaving: discover signals, weave via LLM, emit events.
pub async fn weave_situations(deps: &ScoutEngineDeps, region: Option<&rootsignal_common::ScoutScope>) -> seesaw_core::Events {
    let mut events = seesaw_core::Events::new();

    let (graph, ai, region, budget) = match (
        deps.graph.as_ref(),
        deps.ai.as_deref(),
        region,
        deps.budget.as_ref(),
    ) {
        (Some(g), Some(k), Some(r), Some(b)) => (g, k, r, b),
        _ => return events,
    };
    let run_id = deps.run_id.to_string();

    // Phase 1: Discover unassigned signals
    info!("Starting situation weaving...");
    let signals = match discover::discover_signals(graph, &run_id).await {
        Ok(s) => s,
        Err(e) => {
            warn!(error = %e, "Signal discovery failed");
            return events;
        }
    };

    let signals_discovered = signals.len() as u32;
    if signals.is_empty() {
        info!("SituationWeaver: no unassigned signals, skipping");
        return events;
    }
    info!(count = signals_discovered, "SituationWeaver: discovered unassigned signals");

    // No budget → mark pending and return
    let has_situation_budget = budget.has_budget(OperationCost::CLAUDE_HAIKU_STORY_WEAVE);
    if !has_situation_budget {
        warn!("SituationWeaver: no LLM budget, marking signals as pending");
        let signal_ids: Vec<_> = signals.iter().map(|s| s.id).collect();
        let pending_events = weave::mark_pending(signal_ids, &run_id);
        for e in pending_events {
            events.push(e);
        }
        return events;
    }

    // Phase 2: Load candidates
    let candidates = match discover::load_candidates(graph).await {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "Candidate loading failed");
            return events;
        }
    };
    info!(count = candidates.len(), "SituationWeaver: loaded candidate situations");

    // Phase 3-4: Weave batches via LLM
    let batch_size = 5;
    let mut temp_str_map: std::collections::HashMap<String, uuid::Uuid> =
        std::collections::HashMap::new();
    let mut total_stats = types::SituationWeaverStats::default();
    total_stats.signals_discovered = signals_discovered;

    for chunk in signals.chunks(batch_size) {
        match weave::weave_batch(
            chunk,
            &candidates,
            &mut temp_str_map,
            &deps.embedder,
            ai,
            region,
        )
        .await
        {
            Ok((batch_events, batch_stats)) => {
                total_stats.signals_assigned += batch_stats.signals_assigned;
                total_stats.situations_created += batch_stats.situations_created;
                total_stats.situations_updated += batch_stats.situations_updated;
                total_stats.dispatches_written += batch_stats.dispatches_written;
                total_stats.splits += batch_stats.splits;
                total_stats.merges += batch_stats.merges;
                for e in batch_events {
                    events.push(e);
                }
            }
            Err(e) => {
                warn!(error = %e, "SituationWeaver: batch weaving failed, continuing");
            }
        }
    }

    // Phase 5: Recompute temperature for affected situations
    match discover::find_affected_situations(graph, &run_id).await {
        Ok(affected) => {
            for sit_id in &affected {
                match rootsignal_graph::situation_temperature::compute_temperature_events(
                    graph.client(),
                    sit_id,
                )
                .await
                {
                    Ok((components, temp_events)) => {
                        info!(
                            situation_id = %sit_id,
                            temperature = components.temperature,
                            arc = %components.arc,
                            "Temperature recomputed"
                        );
                        for e in temp_events {
                            events.push(e);
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, situation_id = %sit_id, "Temperature recomputation failed");
                    }
                }
            }
        }
        Err(e) => warn!(error = %e, "Failed to find affected situations"),
    }

    // Phase 6: Post-hoc dispatch verification
    match weave::verify_dispatches(graph).await {
        Ok(verification_events) => {
            total_stats.dispatches_flagged = verification_events.len() as u32;
            for e in verification_events {
                events.push(e);
            }
        }
        Err(e) => warn!(error = %e, "Dispatch verification failed"),
    }

    info!(%total_stats, "SituationWeaver run complete");

    // Situation-driven source boost
    match graph.get_situation_landscape(20).await {
        Ok(situations) => {
            let hot: Vec<_> = situations
                .iter()
                .filter(|s| {
                    s.temperature >= 0.6
                        && s.sensitivity != "SENSITIVE"
                        && s.sensitivity != "RESTRICTED"
                })
                .collect();
            if !hot.is_empty() {
                info!(count = hot.len(), "Hot situations boosting source cadence");
                for sit in &hot {
                    events.push(SystemEvent::SourcesBoostedForSituation {
                        headline: sit.headline.clone(),
                        factor: 1.2,
                    });
                }
            }

            let fuzzy: Vec<_> = situations
                .iter()
                .filter(|s| s.clarity == "Fuzzy" && s.temperature >= 0.3)
                .collect();
            if !fuzzy.is_empty() {
                info!(
                    count = fuzzy.len(),
                    "Fuzzy situations identified for investigation: {}",
                    fuzzy
                        .iter()
                        .map(|s| s.headline.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }
        Err(e) => warn!(error = %e, "Failed to fetch situation landscape for feedback"),
    }

    // Situation-triggered curiosity re-investigation
    match graph.find_curiosity_candidates().await {
        Ok(candidates) if !candidates.is_empty() => {
            info!(count = candidates.len(), "Situations triggered curiosity re-investigation");
            for (situation_id, signal_ids) in candidates {
                events.push(SystemEvent::CuriosityTriggered {
                    situation_id,
                    signal_ids,
                });
            }
        }
        Ok(_) => {}
        Err(e) => warn!(error = %e, "Failed to find curiosity candidates"),
    }

    events
}
