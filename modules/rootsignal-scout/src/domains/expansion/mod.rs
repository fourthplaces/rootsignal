// Signal Expansion domain: follow implied queries to discover additional signals.

pub mod activities;
pub mod events;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::domains::expansion::activities::expansion::Expansion;
use crate::domains::expansion::events::ExpansionEvent;
use crate::domains::scrape::events::ScrapeEvent;

fn is_expansion_ready(e: &ExpansionEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    matches!(e, ExpansionEvent::ExpansionReady)
}

#[handlers]
pub mod handlers {
    use super::*;

    /// ExpansionReady → compute source metrics, expand signals, emit ExpansionCompleted.
    #[handle(on = ExpansionEvent, id = "expansion:expand_signals", filter = is_expansion_ready)]
    async fn expand_signals(
        _event: ExpansionEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let (_, state) = ctx.singleton::<PipelineState>();

        let (graph, budget) = match (deps.graph.as_deref(), deps.budget.as_ref()) {
            (Some(g), Some(b)) => (g, b),
            _ => {
                ctx.logger.debug("Skipped signal expansion: missing graph or budget");
                return Ok(events![ExpansionEvent::ExpansionCompleted {
                    social_expansion_topics: Vec::new(),
                    expansion_deferred_expanded: 0,
                    expansion_queries_collected: 0,
                    expansion_sources_created: 0,
                    expansion_social_topics_queued: 0,
                }]);
            }
        };

        // Source metrics — preamble to expansion
        let mut all_events = if let Some(region) = state.run_scope.region() {
            let all_sources = state
                .source_plan
                .as_ref()
                .map(|s| s.all_sources.clone())
                .unwrap_or_default();
            let source_signal_counts = state.source_signal_counts.clone();
            let query_api_errors = state.query_api_errors.clone();

            crate::domains::enrichment::activities::compute_source_metrics(
                graph,
                &region.name,
                &all_sources,
                &source_signal_counts,
                &query_api_errors,
            )
            .await
        } else {
            Events::new()
        };

        if let Some(ref budget) = deps.budget {
            budget.log_status();
        }

        // Signal expansion
        let region_name = state.run_scope.region().map(|r| r.name.as_str());
        let expansion = Expansion::new(graph, &*deps.embedder);

        let (_, state) = ctx.singleton::<PipelineState>();
        let output = activities::expand_and_discover(
            &expansion,
            Some(deps),
            &state,
            graph,
            region_name,
            deps.ai.as_deref(),
            budget,
            &*deps.embedder,
        )
        .await;

        all_events.extend(output.events);
        all_events.push(ExpansionEvent::ExpansionCompleted {
            social_expansion_topics: output.expansion.social_expansion_topics,
            expansion_deferred_expanded: output.expansion.expansion_deferred_expanded,
            expansion_queries_collected: output.expansion.expansion_queries_collected,
            expansion_sources_created: output.expansion.expansion_sources_created,
            expansion_social_topics_queued: output.expansion.expansion_social_topics_queued,
        });
        if let Some(mut topic_scrape) = output.topic_scrape {
            let scrape_events = topic_scrape.take_events();
            let run_id = deps.run_id;
            all_events.push(ScrapeEvent::SourcesResolved {
                run_id,
                is_response_phase: false,
                web_urls: Vec::new(),
                web_source_keys: Default::default(),
                web_source_count: 0,
                url_mappings: topic_scrape.url_mappings,
                pub_dates: topic_scrape.pub_dates,
                query_api_errors: topic_scrape.query_api_errors,
            });
            all_events.push(ScrapeEvent::TopicDiscoveryCompleted {
                run_id,
                source_signal_counts: topic_scrape.source_signal_counts,
                collected_links: topic_scrape.collected_links,
                expansion_queries: topic_scrape.expansion_queries,
                stats_delta: topic_scrape.stats_delta,
                extracted_batches: Vec::new(),
            });
            all_events.extend(scrape_events);
        }
        Ok(all_events)
    }
}
