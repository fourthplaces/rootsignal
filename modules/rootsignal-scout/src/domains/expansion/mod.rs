// Signal Expansion domain: follow implied queries to discover additional signals.

pub mod activities;
pub mod events;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};



use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::domains::expansion::activities::expansion::Expansion;
use crate::domains::expansion::events::ExpansionEvent;
use crate::domains::scrape::events::{ScrapeEvent, ScrapeRole};
use crate::domains::lifecycle::events::LifecycleEvent;

fn is_metrics_completed(e: &LifecycleEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    matches!(e, LifecycleEvent::MetricsCompleted)
}

#[handlers]
pub mod handlers {
    use super::*;

    /// MetricsCompleted → signal expansion + end-of-run discovery, emit ExpansionCompleted.
    #[handle(on = LifecycleEvent, id = "expansion:signal_expansion", filter = is_metrics_completed)]
    async fn signal_expansion(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();

        // Requires region + graph + budget — skip in tests
        let (region, graph, budget) = match (
            deps.run_scope.region(),
            deps.graph.as_ref(),
            deps.budget.as_ref(),
        ) {
            (Some(r), Some(g), Some(b)) => (r, g, b),
            _ => {
                ctx.logger.debug("Skipped signal expansion: missing region, graph, or budget");
                return Ok(events![ExpansionEvent::ExpansionCompleted {
                    social_expansion_topics: Vec::new(),
                    expansion_deferred_expanded: 0,
                    expansion_queries_collected: 0,
                    expansion_sources_created: 0,
                    expansion_social_topics_queued: 0,
                }]);
            }
        };

        let expansion = Expansion::new(graph, &*deps.embedder, &region.name);

        let (_, state) = ctx.singleton::<PipelineState>();
        let output = activities::expand_and_discover(
            &expansion,
            Some(deps),
            &state,
            &graph,
            &region.name,
            deps.ai.as_deref(),
            budget,
            &*deps.embedder,
        )
        .await;

        // Emit pipeline events instead of direct state writes
        let mut all_events = output.events;
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
                web_role: ScrapeRole::TopicDiscovery,
                web_urls: Vec::new(),
                web_source_keys: Default::default(),
                web_source_count: 0,
                url_mappings: topic_scrape.url_mappings,
                pub_dates: topic_scrape.pub_dates,
                query_api_errors: topic_scrape.query_api_errors,
            });
            all_events.push(ScrapeEvent::ScrapeRoleCompleted {
                run_id,
                role: ScrapeRole::TopicDiscovery,
                urls_scraped: 0,
                urls_unchanged: 0,
                urls_failed: 0,
                signals_extracted: 0,
                source_signal_counts: topic_scrape.source_signal_counts,
                collected_links: topic_scrape.collected_links,
                expansion_queries: topic_scrape.expansion_queries,
                stats_delta: topic_scrape.stats_delta,
                page_previews: Default::default(),
                extracted_batches: Vec::new(),
            });
            all_events.extend(scrape_events);
        }
        Ok(all_events)
    }
}
