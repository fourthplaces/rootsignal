// Discovery domain: finding sources, responses, tensions.

pub mod activities;
pub mod events;

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};
use tracing::info;



use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::core::events::PipelinePhase;
use crate::core::pipeline_events::PipelineEvent;
use crate::domains::discovery::activities::page_triage::{self, PageTriageInput};
use crate::domains::discovery::events::DiscoveryEvent;
use crate::domains::enrichment::activities::link_promoter::{self, PromotionConfig};
use crate::domains::discovery::activities::{bootstrap, discover_expansion_sources};
use rootsignal_common::{canonical_value, DiscoveryMethod, SourceNode, SourceRole};
use rootsignal_common::system_events::SystemEvent;
use crate::domains::lifecycle::events::LifecycleEvent;

fn is_scout_run_requested(e: &LifecycleEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    matches!(e, LifecycleEvent::ScoutRunRequested { .. })
}

fn is_scrape_or_expansion_completed(e: &LifecycleEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    matches!(
        e,
        LifecycleEvent::PhaseCompleted { phase }
            if matches!(phase, PipelinePhase::TensionScrape | PipelinePhase::ResponseScrape | PipelinePhase::SignalExpansion)
    )
}

fn is_tension_scrape_completed(e: &LifecycleEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    matches!(
        e,
        LifecycleEvent::PhaseCompleted { phase }
            if matches!(phase, PipelinePhase::TensionScrape)
    )
}

fn is_sources_discovered(e: &DiscoveryEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    matches!(e, DiscoveryEvent::SourcesDiscovered { .. })
}

#[handlers]
pub mod handlers {
    use super::*;

    /// SourcesDiscovered → filter and gate source registration.
    ///
    /// Auto-accepts social/direct-action/query/admin sources.
    /// LLM-filters web URL sources via `filter_domains_batch`.
    /// Emits `SourceRegistered` (accepted) or `SourceRejected` (audit).
    #[handle(on = DiscoveryEvent, id = "discovery:domain_filter", filter = is_sources_discovered)]
    async fn domain_filter(
        event: DiscoveryEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let DiscoveryEvent::SourcesDiscovered { sources, discovered_by } = event else {
            unreachable!("filter guarantees SourcesDiscovered");
        };

        if sources.is_empty() {
            return Ok(events![PipelineEvent::HandlerSkipped {
                handler_id: "discovery:domain_filter".into(),
                reason: "empty sources batch".into(),
            }]);
        }

        let deps = ctx.deps();
        let region_name = deps.run_scope.region().map(|r| r.name.clone());
        let ai = deps.ai.as_deref();

        let events = activities::domain_filter_gate::filter_discovered_sources(
            sources,
            &discovered_by,
            region_name.as_deref(),
            ai,
            &*deps.store,
        )
        .await;

        Ok(events)
    }

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

    /// PhaseCompleted(TensionScrape|ResponseScrape|SignalExpansion) → promote links from collected pages.
    ///
    /// Two-path gate:
    /// - Social handles: promoted from ALL pages (unchanged behavior)
    /// - Content links: promoted only from "productive" pages (signal_count > 0)
    ///   or pages that pass lightweight LLM triage
    #[handle(on = LifecycleEvent, id = "discovery:link_promotion", filter = is_scrape_or_expansion_completed)]
    async fn link_promotion(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let (_, state) = ctx.singleton::<PipelineState>();

        if state.collected_links.is_empty() {
            return Ok(events![PipelineEvent::HandlerSkipped {
                handler_id: "discovery:link_promotion".into(),
                reason: "no collected links to promote".into(),
            }]);
        }

        let config = PromotionConfig::default();
        let links = state.collected_links.clone();
        let page_previews = state.page_previews.clone();
        let source_signal_counts = state.source_signal_counts.clone();

        // ── 1. Group links by parent page (discovered_on) ──
        let mut links_by_parent: HashMap<String, Vec<String>> = HashMap::new();
        for link in &links {
            links_by_parent
                .entry(link.discovered_on.clone())
                .or_default()
                .push(link.url.clone());
        }

        // ── 2. Partition parents: productive vs needs-triage ──
        let mut productive_pages: HashSet<String> = HashSet::new();
        let mut needs_triage: Vec<String> = Vec::new();

        for parent_url in links_by_parent.keys() {
            // Look up signal count: try canonical key first, fall back to raw URL
            let ck = state.url_to_canonical_key.get(parent_url);
            let signal_count = ck
                .and_then(|k| source_signal_counts.get(k))
                .or_else(|| source_signal_counts.get(parent_url))
                .copied()
                .unwrap_or(0);

            if signal_count > 0 {
                productive_pages.insert(parent_url.clone());
            } else {
                needs_triage.push(parent_url.clone());
            }
        }

        let mut all_events = Events::new();

        // ── 3. Triage zero-signal pages via LLM ──
        if !needs_triage.is_empty() {
            if let Some(ai) = deps.ai.as_deref() {
                let triage_inputs: Vec<PageTriageInput> = needs_triage
                    .iter()
                    .map(|url| {
                        let preview = page_previews.get(url).cloned().unwrap_or_default();
                        let link_count = links_by_parent.get(url).map(|l| l.len()).unwrap_or(0);
                        PageTriageInput {
                            url: url.clone(),
                            content_preview: preview,
                            link_count,
                        }
                    })
                    .collect();

                let verdicts = page_triage::triage_pages(&triage_inputs, ai).await;
                for (url, relevant, reason) in verdicts {
                    all_events.push(DiscoveryEvent::PageTriaged {
                        url: url.clone(),
                        relevant,
                        reason,
                    });
                    if relevant {
                        productive_pages.insert(url);
                    }
                }
            }
            // No AI available → zero-signal pages are not promoted (fail-closed)
        }

        // ── 4. Social handles: promote from ALL pages (unchanged behavior) ──
        let all_urls: Vec<String> = links.iter().map(|l| l.url.clone()).collect();
        let url_to_source: HashMap<String, String> = links
            .iter()
            .map(|l| (l.url.clone(), l.discovered_on.clone()))
            .collect();
        let handles = link_promoter::extract_social_handles_from_links(&all_urls);

        let mut seen = HashSet::new();
        let mut promoted: Vec<(SourceNode, Option<String>)> = Vec::new();
        for (platform, handle) in &handles {
            let url = link_promoter::platform_url(platform, handle);
            let cv = canonical_value(&url);
            if seen.insert(cv.clone()) {
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

        // ── 5. Content links: promote from productive pages only, capped ──
        let social_urls: HashSet<String> = handles
            .iter()
            .map(|(p, h)| link_promoter::platform_url(p, h))
            .collect();

        for (parent_url, child_links) in &links_by_parent {
            if !productive_pages.contains(parent_url) {
                continue;
            }
            let mut content_count = 0usize;
            for link_url in child_links {
                // Skip links that are already promoted as social handles
                if social_urls.contains(link_url) {
                    continue;
                }
                if content_count >= config.max_content_links_per_source {
                    break;
                }
                let cv = canonical_value(link_url);
                if seen.insert(cv.clone()) {
                    let source = SourceNode::new(
                        cv.clone(),
                        cv,
                        Some(link_url.clone()),
                        DiscoveryMethod::LinkedFrom,
                        0.25,
                        SourceRole::Mixed,
                        Some(format!("Linked from {parent_url}")),
                    );
                    promoted.push((source, Some(parent_url.clone())));
                    content_count += 1;
                }
            }
        }

        // ── 6. Emit events ──
        if promoted.is_empty() {
            all_events.push(DiscoveryEvent::LinksPromoted { count: 0 });
            return Ok(all_events);
        }

        let count = promoted.len() as u32;
        info!(count, "Promoting links as sources");

        let mut credit: HashMap<String, u32> = HashMap::new();
        for (_, discovered_on) in &promoted {
            if let Some(parent_url) = discovered_on {
                if let Some(ck) = state.url_to_canonical_key.get(parent_url) {
                    *credit.entry(ck.clone()).or_default() += 1;
                }
            }
        }

        let promoted_sources: Vec<_> = promoted.into_iter().map(|(source, _)| source).collect();
        all_events.push(DiscoveryEvent::SourcesDiscovered {
            sources: promoted_sources,
            discovered_by: "link_promoter".into(),
        });
        for (canonical_key, sources_discovered) in credit {
            all_events.push(SystemEvent::SourceDiscoveryCredit {
                canonical_key,
                sources_discovered,
            });
        }
        all_events.push(DiscoveryEvent::LinksPromoted { count });
        Ok(all_events)
    }

    /// PhaseCompleted(TensionScrape) → expand source pool, emit PhaseCompleted(SourceExpansion).
    #[handle(on = LifecycleEvent, id = "discovery:source_expansion", filter = is_tension_scrape_completed)]
    async fn source_expansion(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        info!("=== Source Expansion ===");
        let deps = ctx.deps();

        // Requires graph + budget — skip in tests
        let (region, graph, budget) = match (
            deps.run_scope.region(),
            deps.graph.as_ref(),
            deps.budget.as_ref(),
        ) {
            (Some(r), Some(g), Some(b)) => (r, g, b),
            _ => {
                ctx.logger.debug("Skipped source expansion: missing region, graph, or budget");
                return Ok(events![LifecycleEvent::PhaseCompleted {
                    phase: PipelinePhase::SourceExpansion,
                }]);
            }
        };

        let output = discover_expansion_sources(
            graph,
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
