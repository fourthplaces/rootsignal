// Lifecycle domain: stale detection, source preparation.

pub mod activities;
pub mod events;

use std::collections::HashMap;

use anyhow::Result;
use causal::{events, reactor, reactors, Context, Events};
use tracing::info;
use uuid::Uuid;

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use events::LifecycleEvent;

fn is_scout_run_requested(e: &LifecycleEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    matches!(e, LifecycleEvent::ScoutRunRequested { .. })
}

fn build_source_keys(sources: &[rootsignal_common::SourceNode]) -> HashMap<String, Uuid> {
    sources.iter().map(|s| (s.canonical_key.clone(), s.id)).collect()
}

#[reactors]
pub mod reactors {
    use super::*;

    /// ScoutRunRequested → find stale signals, emit SignalsExpired.
    #[reactor(on = LifecycleEvent, id = "lifecycle:find_stale_signals", filter = is_scout_run_requested)]
    async fn find_stale_signals(
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
    #[reactor(on = LifecycleEvent, id = "lifecycle:prepare_sources", filter = is_scout_run_requested)]
    async fn prepare_sources(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let state = ctx.aggregate::<PipelineState>().curr;

        // Branch on run modality
        let mut output = match state.run_scope.input_sources() {
            // Source-targeted runs: scrape these specific URLs
            Some(sources) => activities::build_source_plan_from_list(sources),
            // Region runs: load sources from graph, select by cadence
            None => match (state.run_scope.region(), deps.graph.as_deref()) {
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
