// Discovery domain: finding sources, responses, tensions.

pub mod activities;
pub mod events;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};
use tracing::info;

use rootsignal_common::Block;
use rootsignal_common::events::SystemEvent;

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::domains::discovery::activities::link_promotion;
use crate::domains::discovery::events::DiscoveryEvent;
use crate::domains::enrichment::activities::link_promoter::PromotionConfig;
use crate::domains::discovery::activities::{bootstrap, discover_expansion_sources};
use crate::domains::lifecycle::events::LifecycleEvent;
use crate::domains::scrape::events::ScrapeEvent;

fn is_scout_run_requested(e: &LifecycleEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    matches!(e, LifecycleEvent::ScoutRunRequested { .. })
}

fn should_promote_links(event: &ScrapeEvent, ctx: &Context<ScoutEngineDeps>) -> bool {
    if !event.is_completion() {
        return false;
    }
    let state = ctx.aggregate::<PipelineState>().curr;
    if state.collected_links.is_empty() {
        return false;
    }
    if event.is_tension_completion() {
        state.tension_scrape_done()
    } else {
        state.response_scrape_done()
    }
}

fn tension_done_expansion_pending(event: &ScrapeEvent, ctx: &Context<ScoutEngineDeps>) -> bool {
    if !event.is_completion() {
        return false;
    }
    let state = ctx.aggregate::<PipelineState>().curr;
    let result = state.tension_scrape_done() && !state.source_expansion_completed;
    ctx.logger.info(&format!(
        "expand_sources filter: has_plan={}, tension_done={}, expansion_done={}, tension_web={}, tension_social={}, result={}",
        state.source_plan.is_some(),
        state.tension_scrape_done(),
        state.source_expansion_completed,
        state.tension_web_done,
        state.tension_social_done,
        result,
    ));
    result
}

fn describe_promote_links_gate(ctx: &Context<ScoutEngineDeps>) -> Vec<Block> {
    let state = ctx.aggregate::<PipelineState>().curr;
    let plan = state.source_plan.as_ref();
    let mut total = 0u32;
    let mut done = 0u32;
    if plan.is_some_and(|p| p.has_tension_web_sources()) {
        total += 1;
        if state.tension_web_done { done += 1; }
    }
    if plan.is_some_and(|p| p.has_tension_social_sources()) {
        total += 1;
        if state.tension_social_done { done += 1; }
    }
    if plan.is_some() {
        total += 2; // response_web + topic_discovery are always expected
        if state.response_web_done { done += 1; }
        if state.topic_discovery_done { done += 1; }
    }
    if plan.is_some_and(|p| p.has_response_social_sources()) {
        total += 1;
        if state.response_social_done { done += 1; }
    }
    let fraction = if total > 0 { done as f32 / total as f32 } else { 0.0 };
    vec![
        Block::Progress {
            label: "Scrape phases".into(),
            fraction,
        },
        Block::Label {
            text: format!("{} links collected", state.collected_links.len()),
        },
    ]
}

fn describe_expansion_gate(ctx: &Context<ScoutEngineDeps>) -> Vec<Block> {
    let state = ctx.aggregate::<PipelineState>().curr;
    let plan = state.source_plan.as_ref();
    let mut total = 0u32;
    let mut done = 0u32;
    if plan.is_some_and(|p| p.has_tension_web_sources()) {
        total += 1;
        if state.tension_web_done { done += 1; }
    }
    if plan.is_some_and(|p| p.has_tension_social_sources()) {
        total += 1;
        if state.tension_social_done { done += 1; }
    }
    let fraction = if total > 0 { done as f32 / total as f32 } else { 0.0 };
    vec![
        Block::Progress {
            label: "Tension scrape".into(),
            fraction,
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

    /// SourcesDiscovered → filter and register accepted sources.
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
        let state = ctx.aggregate::<PipelineState>().curr;
        let region_name = state.run_scope.region().map(|r| r.name.clone());

        let accepted = activities::domain_filter_gate::filter_discovered_sources(
            sources,
            &discovered_by,
            region_name.as_deref(),
            deps.ai.as_deref(),
            &*deps.store,
            &ctx.logger,
        )
        .await;

        if accepted.is_empty() {
            return Ok(Events::new());
        }
        Ok(events![SystemEvent::SourcesRegistered { sources: accepted }])
    }

    /// ScoutRunRequested → seed sources when the region has none.
    #[handle(on = LifecycleEvent, id = "discovery:bootstrap_sources", filter = is_scout_run_requested)]
    async fn bootstrap_sources(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let state = ctx.aggregate::<PipelineState>().curr;

        let sources = bootstrap::seed_sources_if_empty(&state, deps).await?;
        if sources.is_empty() {
            return Ok(Events::new());
        }
        Ok(events![DiscoveryEvent::SourcesDiscovered {
            sources,
            discovered_by: "engine_started".into(),
        }])
    }

    /// Scrape completed → promote links from collected pages.
    #[handle(on = ScrapeEvent, id = "discovery:promote_links", filter = should_promote_links, describe = describe_promote_links_gate)]
    async fn promote_links(
        _event: ScrapeEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let state = ctx.aggregate::<PipelineState>().curr;

        let result = link_promotion::promote_scraped_links(
            &state.collected_links,
            &state.url_to_canonical_key,
            &state.source_signal_counts,
            &state.page_previews,
            deps.ai.as_deref(),
            &PromotionConfig::default(),
            &ctx.logger,
        ).await;

        let mut all_events = Events::new();
        if !result.sources.is_empty() {
            info!(count = result.sources.len(), "Promoting links as sources");
            all_events.push(DiscoveryEvent::SourcesDiscovered {
                sources: result.sources,
                discovered_by: "link_promoter".into(),
            });
        }
        for (canonical_key, sources_discovered) in result.credit {
            all_events.push(SystemEvent::SourceDiscoveryCredit {
                canonical_key,
                sources_discovered,
            });
        }
        Ok(all_events)
    }

    /// Scrape completed → expand source pool when tension roles done.
    #[handle(on = ScrapeEvent, id = "discovery:expand_sources", filter = tension_done_expansion_pending, describe = describe_expansion_gate)]
    async fn expand_sources(
        _event: ScrapeEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let state = ctx.aggregate::<PipelineState>().curr;

        let (graph, budget) = match (deps.graph.as_deref(), deps.budget.as_ref()) {
            (Some(g), Some(b)) => (g, b),
            _ => {
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

        let mut all_events = Events::new();

        // Register discovered sources
        if !output.sources.is_empty() {
            all_events.push(DiscoveryEvent::SourcesDiscovered {
                sources: output.sources,
                discovered_by: "source_finder".into(),
            });
        }

        // Store query embeddings for future dedup
        for qe in output.query_embeddings {
            all_events.push(SystemEvent::QueryEmbeddingStored {
                canonical_key: qe.canonical_key,
                embedding: qe.embedding,
            });
        }

        if !output.social_topics.is_empty() {
            all_events.push(DiscoveryEvent::SocialTopicsDiscovered {
                topics: output.social_topics,
            });
        }
        all_events.push(DiscoveryEvent::SourceExpansionCompleted);
        Ok(all_events)
    }
}
