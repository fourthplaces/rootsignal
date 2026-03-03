// Signal Expansion domain: follow implied queries to discover additional signals.

pub mod activities;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};

use rootsignal_graph::GraphReader;

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::core::events::PipelinePhase;
use crate::core::pipeline_events::PipelineEvent;
use crate::domains::expansion::activities::expansion::Expansion;
use rootsignal_common::telemetry_events::TelemetryEvent;

use crate::domains::lifecycle::events::LifecycleEvent;
use crate::domains::scrape::activities::Scraper;

fn is_metrics_completed(e: &LifecycleEvent) -> bool {
    matches!(e, LifecycleEvent::MetricsCompleted)
}

#[handlers]
pub mod handlers {
    use super::*;

    /// MetricsCompleted → signal expansion + end-of-run discovery, emit PhaseCompleted(SignalExpansion).
    #[handle(on = LifecycleEvent, id = "expansion:signal_expansion", filter = is_metrics_completed)]
    async fn signal_expansion(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();

        // Requires region + graph_client + budget — skip in tests
        let (region, graph_client, budget) = match (
            deps.region.as_ref(),
            deps.graph_client.as_ref(),
            deps.budget.as_ref(),
        ) {
            (Some(r), Some(g), Some(b)) => (r, g, b),
            _ => {
                let mut skip = events![LifecycleEvent::PhaseCompleted {
                    phase: PipelinePhase::SignalExpansion,
                }];
                skip.push(TelemetryEvent::SystemLog {
                    message: "Skipped signal expansion: missing region, graph_client, or budget".into(),
                    context: Some(serde_json::json!({
                        "handler": "expansion:signal_expansion",
                        "reason": "missing_deps",
                    })),
                });
                return Ok(skip);
            }
        };
        let graph = GraphReader::new(graph_client.clone());

        let expansion = Expansion::new(&graph, &*deps.embedder, &region.name);
        let phase = Scraper::new(
            deps.store.clone(),
            deps.extractor.as_ref().expect("extractor set").clone(),
            deps.fetcher.as_ref().expect("fetcher set").clone(),
        );

        let (_, state) = ctx.singleton::<PipelineState>();
        let output = activities::expand_and_discover(
            &expansion,
            Some(&phase),
            &state,
            &graph,
            &region.name,
            deps.anthropic_api_key.as_deref(),
            budget,
            &*deps.embedder,
        )
        .await;

        // Emit pipeline events instead of direct state writes
        let mut all_events = output.events;
        all_events.push(PipelineEvent::ExpansionAccumulated {
            social_expansion_topics: output.expansion.social_expansion_topics,
            expansion_deferred_expanded: output.expansion.expansion_deferred_expanded,
            expansion_queries_collected: output.expansion.expansion_queries_collected,
            expansion_sources_created: output.expansion.expansion_sources_created,
            expansion_social_topics_queued: output.expansion.expansion_social_topics_queued,
        });
        if let Some(mut topic_scrape) = output.topic_scrape {
            let scrape_events = topic_scrape.take_events();
            all_events.push(PipelineEvent::UrlsResolvedAccumulated {
                url_mappings: topic_scrape.url_mappings,
                pub_dates: topic_scrape.pub_dates,
                query_api_errors: topic_scrape.query_api_errors,
            });
            all_events.push(PipelineEvent::ScrapeResultAccumulated {
                source_signal_counts: topic_scrape.source_signal_counts,
                collected_links: topic_scrape.collected_links,
                expansion_queries: topic_scrape.expansion_queries,
                stats_delta: topic_scrape.stats_delta,
            });
            all_events.extend(scrape_events);
        }
        all_events.push(LifecycleEvent::PhaseCompleted {
            phase: PipelinePhase::SignalExpansion,
        });
        Ok(all_events)
    }
}
