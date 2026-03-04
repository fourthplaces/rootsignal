// Discovery domain: finding sources, responses, tensions.

pub mod activities;
pub mod events;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};
use tracing::info;

use rootsignal_graph::GraphReader;

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::core::events::PipelinePhase;
use crate::core::pipeline_events::PipelineEvent;
use crate::domains::discovery::events::DiscoveryEvent;
use crate::domains::enrichment::activities::domain_filter;
use crate::domains::enrichment::activities::link_promoter::{self, PromotionConfig};
use crate::domains::discovery::activities::{bootstrap, discover_expansion_sources};
use rootsignal_common::telemetry_events::TelemetryEvent;

use crate::domains::lifecycle::events::LifecycleEvent;

fn is_engine_started(e: &LifecycleEvent) -> bool {
    matches!(e, LifecycleEvent::EngineStarted { .. })
}

fn is_scrape_or_expansion_completed(e: &LifecycleEvent) -> bool {
    matches!(
        e,
        LifecycleEvent::PhaseCompleted { phase }
            if matches!(phase, PipelinePhase::TensionScrape | PipelinePhase::ResponseScrape | PipelinePhase::SignalExpansion)
    )
}

fn is_tension_scrape_completed(e: &LifecycleEvent) -> bool {
    matches!(
        e,
        LifecycleEvent::PhaseCompleted { phase }
            if matches!(phase, PipelinePhase::TensionScrape)
    )
}

#[handlers]
pub mod handlers {
    use super::*;

    /// EngineStarted → seed sources when the region has none.
    #[handle(on = LifecycleEvent, id = "discovery:bootstrap", filter = is_engine_started)]
    async fn bootstrap(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let (_, state) = ctx.singleton::<PipelineState>();
        let events = bootstrap::seed_sources_if_empty(&state, deps).await?;
        Ok(events)
    }

    /// PhaseCompleted(TensionScrape|ResponseScrape|SignalExpansion) → promote collected links to sources.
    #[handle(on = LifecycleEvent, id = "discovery:link_promotion", filter = is_scrape_or_expansion_completed)]
    async fn link_promotion(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let (_, state) = ctx.singleton::<PipelineState>();
        if state.collected_links.is_empty() {
            return Ok(events![PipelineEvent::HandlerSkipped {
                handler_id: "discovery:link_promotion".into(),
                reason: "no collected links to promote".into(),
            }]);
        }
        let deps = ctx.deps();
        let links = state.collected_links.clone();

        // Filter out irrelevant domains (SaaS, e-commerce, SEO spam, etc.) via LLM.
        // Fail-open: if api key or region is missing, skip filtering.
        let (links, filter_events) = match (deps.ai.as_ref(), deps.region.as_ref()) {
            (Some(ai), Some(region)) => {
                let urls: Vec<String> = links.iter().map(|l| l.url.clone()).collect();
                let accepted = domain_filter::filter_domains_batch(
                    &urls,
                    &region.name,
                    ai.as_ref(),
                    &*deps.store,
                )
                .await;
                let accepted_set: std::collections::HashSet<&str> =
                    accepted.iter().map(|u| u.as_str()).collect();
                let rejected_urls: Vec<&str> = urls.iter()
                    .map(|u| u.as_str())
                    .filter(|u| !accepted_set.contains(u))
                    .collect();
                let filtered: Vec<_> = links
                    .into_iter()
                    .filter(|l| accepted_set.contains(l.url.as_str()))
                    .collect();
                let rejected = rejected_urls.len();
                let log_event = if rejected > 0 {
                    info!(rejected, accepted = filtered.len(), "Domain filter applied to collected links");
                    Some(TelemetryEvent::SystemLog {
                        message: format!("Domain filter rejected {} of {} links", rejected, rejected + filtered.len()),
                        context: Some(serde_json::json!({
                            "handler": "discovery:link_promotion",
                            "rejected": rejected,
                            "accepted": filtered.len(),
                            "rejected_urls": rejected_urls,
                        })),
                    })
                } else {
                    None
                };
                (filtered, log_event)
            }
            _ => (links, None),
        };

        let promoted = link_promoter::promote_links(&links, &PromotionConfig::default());
        if promoted.is_empty() {
            let mut events = Events::new();
            if let Some(log) = filter_events {
                events.push(log);
            }
            return Ok(events);
        }
        let count = promoted.len() as u32;
        let mut events = Events::new();
        if let Some(log) = filter_events {
            events.push(log);
        }
        for s in promoted {
            events.push(DiscoveryEvent::SourceDiscovered {
                source: s,
                discovered_by: "link_promoter".into(),
            });
        }
        events.push(DiscoveryEvent::LinksPromoted { count });
        Ok(events)
    }

    /// PhaseCompleted(TensionScrape) → expand source pool, emit PhaseCompleted(SourceExpansion).
    #[handle(on = LifecycleEvent, id = "discovery:source_expansion", filter = is_tension_scrape_completed)]
    async fn source_expansion(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        info!("=== Source Expansion ===");
        let deps = ctx.deps();

        // Requires graph_client + budget — skip in tests
        let (region, graph_client, budget) = match (
            deps.region.as_ref(),
            deps.graph_client.as_ref(),
            deps.budget.as_ref(),
        ) {
            (Some(r), Some(g), Some(b)) => (r, g, b),
            _ => {
                let mut skip = events![LifecycleEvent::PhaseCompleted {
                    phase: PipelinePhase::SourceExpansion,
                }];
                skip.push(TelemetryEvent::SystemLog {
                    message: "Skipped source expansion: missing region, graph_client, or budget".into(),
                    context: Some(serde_json::json!({
                        "handler": "discovery:source_expansion",
                        "reason": "missing_deps",
                    })),
                });
                return Ok(skip);
            }
        };
        let graph = GraphReader::new(graph_client.clone());

        let output = discover_expansion_sources(
            &graph,
            &region.name,
            &*deps.embedder,
            deps.ai.as_deref(),
            budget,
        )
        .await;

        // Emit social topics as event instead of direct state write
        let mut all_events = output.events;
        if !output.social_topics.is_empty() {
            all_events.push(PipelineEvent::SocialTopicsCollected {
                topics: output.social_topics,
            });
        }
        all_events.push(LifecycleEvent::PhaseCompleted {
            phase: PipelinePhase::SourceExpansion,
        });
        Ok(all_events)
    }
}
