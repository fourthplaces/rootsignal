// Discovery domain: finding sources, responses, tensions.

pub mod activities;
pub mod events;

use std::collections::HashMap;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};
use tracing::info;

use rootsignal_graph::GraphReader;

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::core::events::PipelinePhase;
use crate::core::pipeline_events::PipelineEvent;
use crate::domains::discovery::events::DiscoveryEvent;
use crate::domains::enrichment::activities::link_promoter;
use crate::domains::discovery::activities::{bootstrap, discover_expansion_sources};
use rootsignal_common::{canonical_value, DiscoveryMethod, SourceNode, SourceRole};
use rootsignal_common::system_events::SystemEvent;
use rootsignal_common::telemetry_events::TelemetryEvent;

use crate::domains::lifecycle::events::LifecycleEvent;

fn is_scout_run_requested(e: &LifecycleEvent) -> bool {
    matches!(e, LifecycleEvent::ScoutRunRequested { .. })
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

    /// ScoutRunRequested → seed sources when the region has none.
    #[handle(on = LifecycleEvent, id = "discovery:bootstrap", filter = is_scout_run_requested)]
    async fn bootstrap(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let (_, state) = ctx.singleton::<PipelineState>();
        let events = bootstrap::seed_sources_if_empty(&state, deps).await?;
        Ok(events)
    }

    /// PhaseCompleted(TensionScrape|ResponseScrape|SignalExpansion) → promote social handles from collected links.
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
        let links = state.collected_links.clone();

        // Extract social handles from collected links and promote as sources.
        // Build a url→discovered_on map for provenance tracking.
        let url_to_source: HashMap<String, String> = links
            .iter()
            .map(|l| (l.url.clone(), l.discovered_on.clone()))
            .collect();
        let all_urls: Vec<String> = links.iter().map(|l| l.url.clone()).collect();
        let handles = link_promoter::extract_social_handles_from_links(&all_urls);

        if handles.is_empty() {
            return Ok(events![
                PipelineEvent::HandlerSkipped {
                    handler_id: "discovery:link_promotion".into(),
                    reason: "no social handles found in collected links".into(),
                },
                DiscoveryEvent::LinksPromoted { count: 0 }
            ]);
        }

        // Dedup by canonical URL and build SourceNodes
        let mut seen = std::collections::HashSet::new();
        let mut promoted: Vec<(SourceNode, Option<String>)> = Vec::new();
        for (platform, handle) in &handles {
            let url = link_promoter::platform_url(platform, handle);
            let cv = canonical_value(&url);
            if seen.insert(cv.clone()) {
                // Find which page this social link was discovered on
                let discovered_on = all_urls.iter()
                    .find(|u| {
                        let u_lower = u.to_lowercase();
                        u_lower.contains(&format!("/{handle}"))
                            || u_lower.contains(&format!("/@{handle}"))
                    })
                    .and_then(|u| url_to_source.get(u))
                    .cloned();
                let gap = discovered_on.as_ref()
                    .map(|src| format!("{platform:?} handle @{handle} found on {src}"))
                    .unwrap_or_else(|| format!("{platform:?} handle @{handle} found on scraped page"));
                let source = SourceNode::new(
                    cv.clone(),
                    cv,
                    Some(url),
                    DiscoveryMethod::LinkedFrom,
                    0.25,
                    SourceRole::Mixed,
                    Some(gap),
                );
                promoted.push((source, discovered_on));
            }
        }

        let count = promoted.len() as u32;
        info!(count, "Promoting social handles as sources");
        let mut events = Events::new();

        // Count discovery credit per parent source
        let mut credit: HashMap<String, u32> = HashMap::new();
        for (_, discovered_on) in &promoted {
            if let Some(parent_url) = discovered_on {
                if let Some(ck) = state.url_to_canonical_key.get(parent_url) {
                    *credit.entry(ck.clone()).or_default() += 1;
                }
            }
        }

        for (source, _) in promoted {
            events.push(DiscoveryEvent::SourceDiscovered {
                source,
                discovered_by: "link_promoter".into(),
            });
        }
        for (canonical_key, sources_discovered) in credit {
            events.push(SystemEvent::SourceDiscoveryCredit {
                canonical_key,
                sources_discovered,
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
            deps.run_scope.region(),
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

        let mut all_events = output.events;
        if !output.social_topics.is_empty() {
            all_events.push(DiscoveryEvent::SocialTopicsDiscovered {
                topics: output.social_topics,
            });
        }
        all_events.push(LifecycleEvent::PhaseCompleted {
            phase: PipelinePhase::SourceExpansion,
        });
        Ok(all_events)
    }
}
