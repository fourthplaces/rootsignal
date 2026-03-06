// Discovery domain: finding sources, responses, tensions.

pub mod activities;
pub mod events;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};
use tracing::info;

use rootsignal_common::{Block, ChecklistItem};

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::domains::discovery::activities::link_promotion;
use crate::domains::discovery::events::DiscoveryEvent;
use crate::domains::enrichment::activities::link_promoter::PromotionConfig;
use crate::domains::discovery::activities::{bootstrap, discover_expansion_sources};
use crate::domains::lifecycle::events::LifecycleEvent;
use crate::domains::scrape::events::{ScrapeEvent, ScrapeRole};

fn is_scout_run_requested(e: &LifecycleEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    matches!(e, LifecycleEvent::ScoutRunRequested { .. })
}

/// Link promotion filter: fires at tension/response phase boundaries
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
    let expected = match role {
        ScrapeRole::TensionWeb | ScrapeRole::TensionSocial => &state.expected_tension_roles,
        _ => &state.expected_response_roles,
    };
    !expected.is_empty() && state.completed_scrape_roles.is_superset(expected)
}

/// Source expansion filter: fires when tension roles done + expansion not yet run.
fn tension_done_expansion_pending(event: &ScrapeEvent, ctx: &Context<ScoutEngineDeps>) -> bool {
    if event.completed_role().is_none() {
        return false;
    }
    let (_, state) = ctx.singleton::<PipelineState>();
    !state.expected_tension_roles.is_empty()
        && state.completed_scrape_roles.is_superset(&state.expected_tension_roles)
        && !state.source_expansion_completed
}

fn describe_promote_links_gate(ctx: &Context<ScoutEngineDeps>) -> Vec<Block> {
    let (_, state) = ctx.singleton::<PipelineState>();
    let done = &state.completed_scrape_roles;
    let link_count = state.collected_links.len() as u32;
    vec![
        Block::Checklist {
            label: "Tension scrape".into(),
            items: state.expected_tension_roles.iter().map(|r| ChecklistItem {
                text: format!("{r:?}"),
                done: done.contains(r),
            }).collect(),
        },
        Block::Checklist {
            label: "Response scrape".into(),
            items: state.expected_response_roles.iter().map(|r| ChecklistItem {
                text: format!("{r:?}"),
                done: done.contains(r),
            }).collect(),
        },
        Block::Counter {
            label: "Collected links".into(),
            value: link_count,
            total: link_count,
        },
    ]
}

fn describe_expansion_gate(ctx: &Context<ScoutEngineDeps>) -> Vec<Block> {
    let (_, state) = ctx.singleton::<PipelineState>();
    let done = &state.completed_scrape_roles;
    vec![
        Block::Checklist {
            label: "Tension scrape".into(),
            items: state.expected_tension_roles.iter().map(|r| ChecklistItem {
                text: format!("{r:?}"),
                done: done.contains(r),
            }).collect(),
        },
        Block::Status {
            label: "Source expansion".into(),
            state: if state.source_expansion_completed {
                rootsignal_common::State::Done
            } else {
                rootsignal_common::State::Waiting
            },
        },
    ]
}

#[handlers]
pub mod handlers {
    use super::*;

    /// SourcesDiscovered → filter and gate source registration.
    ///
    /// Auto-accepts social/direct-action/query/admin sources.
    /// LLM-filters web URL sources via `filter_domains_batch`.
    /// Emits `SourcesRegistered` for accepted sources; rejections are logged.
    #[handle(on = [DiscoveryEvent::SourcesDiscovered], id = "discovery:filter_domains", extract(sources, discovered_by))]
    async fn filter_domains(
        sources: Vec<rootsignal_common::SourceNode>,
        discovered_by: String,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {

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
            &ctx.logger,
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
    #[handle(on = ScrapeEvent, id = "discovery:promote_links", filter = should_promote_links, describe = describe_promote_links_gate)]
    async fn promote_links(
        _event: ScrapeEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let (_, state) = ctx.singleton::<PipelineState>();

        let result = link_promotion::promote_scraped_links(
            &state.collected_links,
            &state.url_to_canonical_key,
            &state.source_signal_counts,
            &state.page_previews,
            deps.ai.as_deref(),
            &PromotionConfig::default(),
        ).await;

        Ok(result.into_events())
    }

    /// Scrape completed → expand source pool when tension roles done.
    /// Emits SourceExpansionCompleted or SourceExpansionSkipped.
    #[handle(on = ScrapeEvent, id = "discovery:expand_sources", filter = tension_done_expansion_pending, describe = describe_expansion_gate)]
    async fn expand_sources(
        _event: ScrapeEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        info!("=== Source Expansion ===");
        let deps = ctx.deps();
        let (_, state) = ctx.singleton::<PipelineState>();

        let (graph, budget) = match (deps.graph.as_ref(), deps.budget.as_ref()) {
            (Some(g), Some(b)) => (g, b),
            _ => {
                ctx.logger.debug("Skipped source expansion: missing graph or budget");
                return Ok(events![DiscoveryEvent::SourceExpansionSkipped {
                    reason: "missing graph or budget".into(),
                }]);
            }
        };
        let region_name = state.run_scope.region().map(|r| r.name.as_str());

        let output = discover_expansion_sources(
            graph,
            region_name,
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
