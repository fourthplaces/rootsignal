// Discovery domain: finding sources, responses, tensions.

pub mod activities;
pub mod events;

use std::collections::HashSet;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};
use tracing::info;

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::domains::discovery::activities::link_promotion;
use crate::domains::discovery::activities::page_triage::{self, PageTriageInput};
use crate::domains::discovery::events::DiscoveryEvent;
use crate::domains::enrichment::activities::link_promoter::PromotionConfig;
use crate::domains::discovery::activities::{bootstrap, discover_expansion_sources};
use rootsignal_common::system_events::SystemEvent;
use crate::domains::lifecycle::events::LifecycleEvent;
use crate::domains::scrape::events::{ScrapeEvent, ScrapeRole};

fn is_scout_run_requested(e: &LifecycleEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    matches!(e, LifecycleEvent::ScoutRunRequested { .. })
}

fn is_sources_discovered(e: &DiscoveryEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    matches!(e, DiscoveryEvent::SourcesDiscovered { .. })
}

/// Expected roles for each scrape phase, used for completion tracking.
fn tension_roles() -> HashSet<ScrapeRole> {
    HashSet::from([ScrapeRole::TensionWeb, ScrapeRole::TensionSocial])
}

fn response_roles() -> HashSet<ScrapeRole> {
    HashSet::from([ScrapeRole::ResponseWeb, ScrapeRole::ResponseSocial, ScrapeRole::TopicDiscovery])
}

/// Link promotion filter: fires at tension/response/expansion phase boundaries
/// when there are links to promote.
fn should_promote_links(event: &ScrapeEvent, ctx: &Context<ScoutEngineDeps>) -> bool {
    let role = match event.completed_role() {
        Some(r) => r,
        None => return false,
    };
    let (_, state) = ctx.singleton::<PipelineState>();
    if state.collected_links.is_empty() {
        return false;
    }
    match role {
        ScrapeRole::TensionWeb | ScrapeRole::TensionSocial =>
            state.completed_scrape_roles.is_superset(&tension_roles()),
        _ => state.completed_scrape_roles.is_superset(&response_roles()),
    }
}

/// Source expansion filter: fires when tension roles done + expansion not yet run.
fn tension_done_expansion_pending(event: &ScrapeEvent, ctx: &Context<ScoutEngineDeps>) -> bool {
    if event.completed_role().is_none() {
        return false;
    }
    let (_, state) = ctx.singleton::<PipelineState>();
    state.completed_scrape_roles.is_superset(&tension_roles())
        && !state.source_expansion_completed
}

#[handlers]
pub mod handlers {
    use super::*;

    /// SourcesDiscovered → filter and gate source registration.
    ///
    /// Auto-accepts social/direct-action/query/admin sources.
    /// LLM-filters web URL sources via `filter_domains_batch`.
    /// Emits `SourceRegistered` (accepted) or `SourceRejected` (audit).
    #[handle(on = DiscoveryEvent, id = "discovery:filter_domains", filter = is_sources_discovered)]
    async fn filter_domains(
        event: DiscoveryEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let DiscoveryEvent::SourcesDiscovered { sources, discovered_by } = event else {
            unreachable!("filter guarantees SourcesDiscovered");
        };

        if sources.is_empty() {
            return Ok(Events::new());
        }

        let deps = ctx.deps();
        let (_, state) = ctx.singleton::<PipelineState>();
        let region_name = state.run_scope.region().map(|r| r.name.clone());
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
    #[handle(on = LifecycleEvent, id = "discovery:bootstrap_sources", filter = is_scout_run_requested)]
    async fn bootstrap_sources(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let (_, state) = ctx.singleton::<PipelineState>();
        let events = bootstrap::seed_sources_if_empty(&state, deps).await?;
        Ok(events)
    }

    /// Scrape completed → promote links from collected pages.
    ///
    /// Filter gates on: tension_roles or response_roles done + links not empty.
    /// Two-path gate:
    /// - Social handles: promoted from ALL pages (unchanged behavior)
    /// - Content links: promoted only from "productive" pages (signal_count > 0)
    ///   or pages that pass lightweight LLM triage
    #[handle(on = ScrapeEvent, id = "discovery:promote_links", filter = should_promote_links)]
    async fn promote_links(
        _event: ScrapeEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let (_, state) = ctx.singleton::<PipelineState>();

        let config = PromotionConfig::default();
        let links = state.collected_links.clone();

        let links_by_parent = link_promotion::group_links_by_parent(&links);

        let (mut productive_pages, needs_triage) = link_promotion::classify_parent_pages(
            links_by_parent.keys().map(|s| s.as_str()),
            &state.url_to_canonical_key,
            &state.source_signal_counts,
        );

        // Triage zero-signal pages via LLM (fail-closed when no AI)
        let mut all_events = Events::new();
        if !needs_triage.is_empty() {
            if let Some(ai) = deps.ai.as_deref() {
                let triage_inputs: Vec<PageTriageInput> = needs_triage
                    .iter()
                    .map(|url| PageTriageInput {
                        url: url.clone(),
                        content_preview: state.page_previews.get(url).cloned().unwrap_or_default(),
                        link_count: links_by_parent.get(url).map(|l| l.len()).unwrap_or(0),
                    })
                    .collect();

                for (url, relevant, reason) in page_triage::triage_pages(&triage_inputs, ai).await {
                    ctx.logger.debug(&format!("page triage: {url} → relevant={relevant}, {reason}"));
                    if relevant {
                        productive_pages.insert(url);
                    }
                }
            }
        }

        let (social, social_urls) = link_promotion::promote_social_handles(&links);
        let content = link_promotion::promote_content_links(
            &links_by_parent,
            &productive_pages,
            &social_urls,
            &config,
        );

        let mut all_promoted: Vec<_> = social.into_iter().chain(content).collect();

        if all_promoted.is_empty() {
            all_events.push(DiscoveryEvent::LinksPromoted { count: 0 });
            return Ok(all_events);
        }

        let count = all_promoted.len() as u32;
        info!(count, "Promoting links as sources");

        let credit = link_promotion::compute_discovery_credit(&all_promoted, &state.url_to_canonical_key);

        let sources: Vec<_> = all_promoted.drain(..).map(|p| p.source).collect();
        all_events.push(DiscoveryEvent::SourcesDiscovered {
            sources,
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

    /// Scrape completed → expand source pool when tension roles done.
    /// Emits SourceExpansionCompleted or SourceExpansionSkipped.
    #[handle(on = ScrapeEvent, id = "discovery:expand_sources", filter = tension_done_expansion_pending)]
    async fn expand_sources(
        _event: ScrapeEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        info!("=== Source Expansion ===");
        let deps = ctx.deps();
        let (_, state) = ctx.singleton::<PipelineState>();

        // Requires graph + budget — skip in tests
        let (region, graph, budget) = match (
            state.run_scope.region(),
            deps.graph.as_ref(),
            deps.budget.as_ref(),
        ) {
            (Some(r), Some(g), Some(b)) => (r, g, b),
            _ => {
                ctx.logger.debug("Skipped source expansion: missing region, graph, or budget");
                return Ok(events![DiscoveryEvent::SourceExpansionSkipped {
                    reason: "missing region, graph, or budget".into(),
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
        all_events.push(DiscoveryEvent::SourceExpansionCompleted);
        Ok(all_events)
    }
}
