// Lifecycle domain: stale detection, source preparation, finalize.

pub mod activities;
pub mod events;

use std::collections::HashMap;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};
use tracing::info;
use uuid::Uuid;

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::domains::expansion::events::ExpansionEvent;
use crate::domains::supervisor::events::SupervisorEvent;
use crate::domains::synthesis::events::{all_synthesis_roles, SynthesisEvent};
use events::LifecycleEvent;

fn is_scout_run_requested(e: &LifecycleEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    matches!(e, LifecycleEvent::ScoutRunRequested { .. })
}

fn all_synthesis_done(e: &SynthesisEvent, ctx: &Context<ScoutEngineDeps>) -> bool {
    if !matches!(e, SynthesisEvent::SynthesisRoleCompleted { .. }) { return false; }
    let (_, state) = ctx.singleton::<PipelineState>();
    state.completed_synthesis_roles.is_superset(&all_synthesis_roles())
}

fn is_supervision_done(e: &SupervisorEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    matches!(
        e,
        SupervisorEvent::SupervisionCompleted | SupervisorEvent::NothingToSupervise { .. }
    )
}

fn build_source_keys(sources: &[rootsignal_common::SourceNode]) -> HashMap<String, Uuid> {
    sources.iter().map(|s| (s.canonical_key.clone(), s.id)).collect()
}

#[handlers]
pub mod handlers {
    use super::*;

    /// ScoutRunRequested → find stale signals, emit SignalsExpired.
    #[handle(on = LifecycleEvent, id = "lifecycle:find_stale", filter = is_scout_run_requested)]
    async fn find_stale(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let stale = activities::find_stale_signals(&*deps.store).await;

        if stale.is_empty() {
            return Ok(Events::new());
        }

        Ok(events![rootsignal_common::events::SystemEvent::SignalsExpired {
            signals: stale,
        }])
    }

    /// ScoutRunRequested → build source plan, resolve web URLs, emit SourcesPrepared.
    #[handle(on = LifecycleEvent, id = "lifecycle:prepare_sources", filter = is_scout_run_requested)]
    async fn prepare_sources(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let (_, state) = ctx.singleton::<PipelineState>();

        // Branch on run modality
        let mut output = match state.run_scope.input_sources() {
            // Source-targeted runs: scrape these specific URLs
            Some(sources) => activities::build_source_plan_from_list(sources),
            // Region runs: load sources from graph, select by cadence
            None => match (state.run_scope.region(), deps.graph.as_ref()) {
                (Some(region), Some(graph)) => {
                    activities::build_source_plan_from_region(graph, region).await
                }
                _ => {
                    ctx.logger.warn("No region or graph available, skipping source plan");
                    return Ok(events![]);
                }
            },
        };

        info!("=== Phase A: Find Problems ===");

        let web_sources: Vec<rootsignal_common::SourceNode> = output.source_plan
            .selected_sources
            .iter()
            .filter(|s| {
                output.source_plan.tension_phase_keys.contains(&s.canonical_key)
                    && !matches!(
                        rootsignal_common::scraping_strategy(s.value()),
                        rootsignal_common::ScrapingStrategy::Social(_)
                    )
            })
            .cloned()
            .collect();

        let web_source_refs: Vec<&rootsignal_common::SourceNode> = web_sources.iter().collect();
        let resolution = crate::domains::scrape::activities::url_resolution::resolve_web_urls(
            deps,
            &web_source_refs,
            &output.url_mappings,
        ).await;

        let web_source_keys = build_source_keys(&web_sources);

        // Merge resolution url_mappings (redirects) into source url_mappings
        output.url_mappings.extend(resolution.url_mappings);

        Ok(events![
            LifecycleEvent::SourcesPrepared {
                tension_count: output.tension_count,
                response_count: output.response_count,
                source_plan: output.source_plan,
                actor_contexts: output.actor_contexts,
                url_mappings: output.url_mappings,
                web_urls: resolution.urls,
                web_source_keys,
                web_source_count: resolution.source_count,
                pub_dates: resolution.pub_dates,
                query_api_errors: resolution.query_api_errors,
            },
        ])
    }
}

// ---------------------------------------------------------------------------
// Standalone finalize handlers — one per engine variant
// ---------------------------------------------------------------------------

async fn finalize_impl(ctx: Context<ScoutEngineDeps>) -> Result<Events> {
    let deps = ctx.deps();

    let (_, state) = ctx.singleton::<PipelineState>();
    let stats = state.stats.clone();

    if let Some(ref budget) = deps.budget {
        budget.log_status();
    }

    info!("{}", stats);
    Ok(events![LifecycleEvent::RunCompleted { stats }])
}

/// Finalize handler for the scrape chain: triggers when all synthesis roles done.
#[handle(on = SynthesisEvent, id = "lifecycle:scrape_finalize", filter = all_synthesis_done)]
pub async fn scrape_finalize(
    _event: SynthesisEvent,
    ctx: Context<ScoutEngineDeps>,
) -> Result<Events> {
    finalize_impl(ctx).await
}

/// Finalize handler for the full chain: triggers on SupervisionCompleted or NothingToSupervise.
#[handle(on = SupervisorEvent, id = "lifecycle:full_finalize", filter = is_supervision_done)]
pub async fn full_finalize(
    _event: SupervisorEvent,
    ctx: Context<ScoutEngineDeps>,
) -> Result<Events> {
    finalize_impl(ctx).await
}

/// Kickoff handler for weave engine: emits ExpansionCompleted on ScoutRunRequested.
/// The weave engine skips scrape/enrichment/expansion, so this provides the
/// trigger that synthesis handlers need to start.
#[handle(on = LifecycleEvent, id = "lifecycle:weave_kickoff", filter = is_scout_run_requested)]
pub async fn weave_kickoff(
    _event: LifecycleEvent,
    _ctx: Context<ScoutEngineDeps>,
) -> Result<Events> {
    Ok(events![ExpansionEvent::ExpansionCompleted {
        social_expansion_topics: Vec::new(),
        expansion_deferred_expanded: 0,
        expansion_queries_collected: 0,
        expansion_sources_created: 0,
        expansion_social_topics_queued: 0,
    }])
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use seesaw_core::AnyEvent;

    use crate::core::engine::{build_engine, ScoutEngineDeps};
    use crate::domains::synthesis::events::{SynthesisEvent, SynthesisRole};

    fn build_test_engine() -> (seesaw_core::Engine<ScoutEngineDeps>, Arc<Mutex<Vec<AnyEvent>>>) {
        let sink = Arc::new(Mutex::new(Vec::new()));
        let mut deps = ScoutEngineDeps::new(
            Arc::new(crate::testing::MockSignalReader::new()),
            Arc::new(crate::infra::embedder::NoOpEmbedder),
            uuid::Uuid::new_v4(),
        );
        deps.captured_events = Some(sink.clone());
        let engine = build_engine(deps, None);
        (engine, sink)
    }

    fn all_synthesis_role_completed_events() -> Vec<SynthesisEvent> {
        use crate::domains::synthesis::events::all_synthesis_roles;
        let run_id = uuid::Uuid::new_v4();
        all_synthesis_roles().into_iter().map(|role| {
            SynthesisEvent::SynthesisRoleCompleted {
                run_id,
                role,
            }
        }).collect()
    }

    #[tokio::test]
    async fn finalize_emits_run_completed() {
        let (engine, sink) = build_test_engine();

        // Emit all synthesis role completions to trigger finalize
        for event in all_synthesis_role_completed_events() {
            engine.emit(event).settled().await.unwrap();
        }

        let events = sink.lock().unwrap();
        let has_run_completed = events
            .iter()
            .any(|e| e.downcast_ref::<crate::domains::lifecycle::events::LifecycleEvent>().is_some_and(|le| matches!(le, crate::domains::lifecycle::events::LifecycleEvent::RunCompleted { .. })));

        assert!(has_run_completed, "should emit RunCompleted");
    }

    #[tokio::test]
    async fn handler_failure_counted_in_run_completed_stats() {
        use crate::core::pipeline_events::PipelineEvent;

        let (engine, sink) = build_test_engine();

        engine
            .emit(PipelineEvent::HandlerFailed {
                handler_id: "scrape:fetch".to_string(),
                source_event_type: "ScrapeEvent".to_string(),
                error: "connection timeout".to_string(),
                attempts: 3,
            })
            .settled()
            .await
            .unwrap();

        engine
            .emit(PipelineEvent::HandlerFailed {
                handler_id: "synthesis:linker".to_string(),
                source_event_type: "SynthesisEvent".to_string(),
                error: "panicked at 'index out of bounds'".to_string(),
                attempts: 1,
            })
            .settled()
            .await
            .unwrap();

        // Trigger finalize — emit all synthesis role completions
        for event in all_synthesis_role_completed_events() {
            engine.emit(event).settled().await.unwrap();
        }

        let events = sink.lock().unwrap();
        let stats = events
            .iter()
            .filter_map(|e| e.downcast_ref::<crate::domains::lifecycle::events::LifecycleEvent>())
            .find_map(|le| match le {
                crate::domains::lifecycle::events::LifecycleEvent::RunCompleted { stats } => Some(stats),
                _ => None,
            })
            .expect("should emit RunCompleted");

        assert_eq!(stats.handler_failures, 2, "RunCompleted stats should carry accumulated handler failure count");
    }
}
