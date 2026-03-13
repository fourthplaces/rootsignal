pub mod activities;
pub mod events;

#[cfg(test)]
mod boundary_tests;
#[cfg(test)]
mod chain_tests;
#[cfg(test)]
pub mod simweb_adapter;

use std::collections::HashMap;

use anyhow::Result;
use causal::{events, reactor, reactors, Context, Events};
use tracing::info;
use uuid::Uuid;

use rootsignal_common::Block;

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::domains::discovery::events::DiscoveryEvent;
use crate::domains::lifecycle::events::LifecycleEvent;

use crate::domains::scrape::events::ScrapeEvent;

fn has_tension_web_sources(e: &LifecycleEvent, ctx: &Context<ScoutEngineDeps>) -> bool {
    if !matches!(e, LifecycleEvent::SourcesPrepared { .. }) {
        return false;
    }
    let state = ctx.aggregate::<PipelineState>().curr;
    state.source_plan.as_ref().is_some_and(|p| p.has_tension_web_sources())
}

fn has_tension_social_sources(e: &LifecycleEvent, ctx: &Context<ScoutEngineDeps>) -> bool {
    if !matches!(e, LifecycleEvent::SourcesPrepared { .. }) {
        return false;
    }
    let state = ctx.aggregate::<PipelineState>().curr;
    state.source_plan.as_ref().is_some_and(|p| p.has_tension_social_sources())
}

fn has_response_social_sources(e: &ScrapeEvent, ctx: &Context<ScoutEngineDeps>) -> bool {
    if !matches!(e, ScrapeEvent::SourcesResolved { is_response_phase: true, .. }) {
        return false;
    }
    let state = ctx.aggregate::<PipelineState>().curr;
    state.source_plan.as_ref().is_some_and(|p| p.has_response_social_sources())
}

fn is_source_expansion_done(e: &DiscoveryEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    matches!(
        e,
        DiscoveryEvent::SourceExpansionCompleted | DiscoveryEvent::SourceExpansionSkipped { .. }
    )
}

fn is_response_sources_resolved(e: &ScrapeEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    matches!(e, ScrapeEvent::SourcesResolved { is_response_phase: true, .. })
}

fn describe_resolve_new_source_gate(ctx: &Context<ScoutEngineDeps>) -> Vec<Block> {
    let state = ctx.aggregate::<PipelineState>().curr;
    vec![
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

/// Build source_keys (canonical_key → source_id) from filtered sources.
fn build_source_keys(sources: &[rootsignal_common::SourceNode]) -> HashMap<String, Uuid> {
    sources.iter().map(|s| (s.canonical_key.clone(), s.id)).collect()
}

#[reactors]
pub mod reactors {
    use super::*;

    // -----------------------------------------------------------------------
    // Phase A handlers — triggered by SourcesPrepared
    // -----------------------------------------------------------------------

    /// SourcesPrepared → fetch + extract web pages (only if plan has non-social sources).
    #[reactor(on = LifecycleEvent, id = "scrape:start_web_scrape", filter = has_tension_web_sources)]
    async fn start_web_scrape(
        event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let (web_urls, web_source_keys) = match event {
            LifecycleEvent::SourcesPrepared { web_urls, web_source_keys, .. } => (web_urls, web_source_keys),
            _ => unreachable!("filter guarantees SourcesPrepared"),
        };

        let run_id = ctx.deps().run_id;

        info!(url_count = web_urls.len(), "Fetching web pages");

        if web_urls.is_empty() {
            return Ok(events![ScrapeEvent::WebScrapeCompleted {
                run_id,
                is_tension: true,
                urls_scraped: 0,
                urls_unchanged: 0,
                urls_failed: 0,
                signals_extracted: 0,
                signals_rejected: 0,
                source_signal_counts: Default::default(),
                collected_links: Default::default(),
                expansion_queries: Default::default(),
                page_previews: Default::default(),
                extracted_batches: Default::default(),
            }]);
        }

        let deps = ctx.deps();
        let state = ctx.aggregate::<PipelineState>().curr;

        let fetch_result = activities::web_scrape::fetch_and_extract(
            deps,
            &web_urls,
            &web_source_keys,
            &state.url_to_canonical_key,
            &state.actor_contexts,
            &state.url_to_pub_date,
            &ctx.logger,
        ).await;

        let mut all_events = Events::new();
        all_events.extend(fetch_result.events);
        all_events.push(ScrapeEvent::WebScrapeCompleted {
            run_id,
            is_tension: true,
            urls_scraped: fetch_result.stats.urls_scraped,
            urls_unchanged: fetch_result.stats.urls_unchanged,
            urls_failed: fetch_result.stats.urls_failed,
            signals_extracted: fetch_result.stats.signals_extracted,
            signals_rejected: fetch_result.stats.signals_rejected,
            source_signal_counts: fetch_result.source_signal_counts,
            collected_links: fetch_result.collected_links,
            expansion_queries: fetch_result.expansion_queries,
            page_previews: fetch_result.page_previews,
            extracted_batches: fetch_result.extracted_batches,
        });

        Ok(all_events)
    }

    /// SourcesPrepared → fetch + extract social media posts (only if plan has social sources).
    #[reactor(on = LifecycleEvent, id = "scrape:start_social_scrape", filter = has_tension_social_sources)]
    async fn start_social_scrape(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let run_id = ctx.deps().run_id;

        info!("Fetching social media posts");

        let deps = ctx.deps();
        let state = ctx.aggregate::<PipelineState>().curr;
        let plan = state.source_plan.as_ref().expect("source plan stashed");

        let social_sources: Vec<&rootsignal_common::SourceNode> = plan.selected_sources
            .iter()
            .filter(|s| {
                matches!(
                    rootsignal_common::scraping_strategy(s.value()),
                    rootsignal_common::ScrapingStrategy::Social(_)
                ) && plan.tension_phase_keys.contains(&s.canonical_key)
            })
            .collect();

        if social_sources.is_empty() {
            return Ok(events![ScrapeEvent::SocialScrapeCompleted {
                run_id,
                is_tension: true,
                sources_scraped: 0,
                signals_extracted: 0,
                signals_rejected: 0,
                source_signal_counts: Default::default(),
                collected_links: Default::default(),
                expansion_queries: Default::default(),
                stats_delta: Default::default(),
                extracted_batches: Default::default(),
            }]);
        }

        let mut social_output = activities::social_scrape::scrape_social_sources(
            deps,
            &social_sources,
            &state.url_to_canonical_key,
            &state.actor_contexts,
            &ctx.logger,
        ).await;

        let events = social_output.take_events();
        let signals_extracted: u32 = social_output.source_signal_counts.values().sum();

        let mut all_events = Events::new();
        all_events.extend(events);
        all_events.push(ScrapeEvent::SocialScrapeCompleted {
            run_id,
            is_tension: true,
            sources_scraped: social_sources.len() as u32,
            signals_extracted,
            signals_rejected: 0,
            source_signal_counts: social_output.source_signal_counts,
            collected_links: social_output.collected_links,
            expansion_queries: social_output.expansion_queries,
            stats_delta: social_output.stats_delta,
            extracted_batches: social_output.extracted_batches,
        });

        Ok(all_events)
    }

    // -----------------------------------------------------------------------
    // Phase B handlers — triggered by SourcesResolved (response phase)
    // -----------------------------------------------------------------------

    /// SourcesResolved → fetch + extract response web pages.
    #[reactor(on = ScrapeEvent, id = "scrape:process_web_results", filter = is_response_sources_resolved)]
    async fn process_web_results(
        event: ScrapeEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let (run_id, web_urls, web_source_keys) = match event {
            ScrapeEvent::SourcesResolved { run_id, web_urls, web_source_keys, .. } => (run_id, web_urls, web_source_keys),
            _ => unreachable!("filter guarantees SourcesResolved"),
        };

        info!(url_count = web_urls.len(), "Fetching response web pages");

        if web_urls.is_empty() {
            return Ok(events![ScrapeEvent::WebScrapeCompleted {
                run_id,
                is_tension: false,
                urls_scraped: 0,
                urls_unchanged: 0,
                urls_failed: 0,
                signals_extracted: 0,
                signals_rejected: 0,
                source_signal_counts: Default::default(),
                collected_links: Default::default(),
                expansion_queries: Default::default(),
                page_previews: Default::default(),
                extracted_batches: Default::default(),
            }]);
        }

        let deps = ctx.deps();
        let state = ctx.aggregate::<PipelineState>().curr;

        let fetch_result = activities::web_scrape::fetch_and_extract(
            deps,
            &web_urls,
            &web_source_keys,
            &state.url_to_canonical_key,
            &state.actor_contexts,
            &state.url_to_pub_date,
            &ctx.logger,
        ).await;

        let mut all_events = Events::new();
        all_events.extend(fetch_result.events);
        all_events.push(ScrapeEvent::WebScrapeCompleted {
            run_id,
            is_tension: false,
            urls_scraped: fetch_result.stats.urls_scraped,
            urls_unchanged: fetch_result.stats.urls_unchanged,
            urls_failed: fetch_result.stats.urls_failed,
            signals_extracted: fetch_result.stats.signals_extracted,
            signals_rejected: fetch_result.stats.signals_rejected,
            source_signal_counts: fetch_result.source_signal_counts,
            collected_links: fetch_result.collected_links,
            expansion_queries: fetch_result.expansion_queries,
            page_previews: fetch_result.page_previews,
            extracted_batches: fetch_result.extracted_batches,
        });

        Ok(all_events)
    }

    /// SourcesResolved → fetch + extract response social media posts (only if plan has social sources).
    #[reactor(on = ScrapeEvent, id = "scrape:process_social_results", filter = has_response_social_sources)]
    async fn process_social_results(
        _event: ScrapeEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let run_id = ctx.deps().run_id;

        info!("Fetching response social media posts");

        let deps = ctx.deps();
        let state = ctx.aggregate::<PipelineState>().curr;
        let plan = state.source_plan.as_ref().expect("source plan stashed");

        let social_sources: Vec<&rootsignal_common::SourceNode> = plan.selected_sources
            .iter()
            .filter(|s| {
                matches!(
                    rootsignal_common::scraping_strategy(s.value()),
                    rootsignal_common::ScrapingStrategy::Social(_)
                ) && plan.response_phase_keys.contains(&s.canonical_key)
            })
            .collect();

        if social_sources.is_empty() {
            return Ok(events![ScrapeEvent::SocialScrapeCompleted {
                run_id,
                is_tension: false,
                sources_scraped: 0,
                signals_extracted: 0,
                signals_rejected: 0,
                source_signal_counts: Default::default(),
                collected_links: Default::default(),
                expansion_queries: Default::default(),
                stats_delta: Default::default(),
                extracted_batches: Default::default(),
            }]);
        }

        let mut social_output = activities::social_scrape::scrape_social_sources(
            deps,
            &social_sources,
            &state.url_to_canonical_key,
            &state.actor_contexts,
            &ctx.logger,
        ).await;

        let events = social_output.take_events();
        let signals_extracted: u32 = social_output.source_signal_counts.values().sum();

        let mut all_events = Events::new();
        all_events.extend(events);
        all_events.push(ScrapeEvent::SocialScrapeCompleted {
            run_id,
            is_tension: false,
            sources_scraped: social_sources.len() as u32,
            signals_extracted,
            signals_rejected: 0,
            source_signal_counts: social_output.source_signal_counts,
            collected_links: social_output.collected_links,
            expansion_queries: social_output.expansion_queries,
            stats_delta: social_output.stats_delta,
            extracted_batches: social_output.extracted_batches,
        });

        Ok(all_events)
    }

    /// SourcesResolved → discover from social topics.
    #[reactor(on = ScrapeEvent, id = "scrape:discover_topics", filter = is_response_sources_resolved)]
    async fn discover_topics(
        event: ScrapeEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let run_id = match &event {
            ScrapeEvent::SourcesResolved { run_id, .. } => *run_id,
            _ => unreachable!("filter guarantees SourcesResolved"),
        };

        info!("Fetch topics for topic discovery");

        let deps = ctx.deps();

        let state = ctx.aggregate::<PipelineState>().curr;

        let mut all_social_topics = state.social_topics.clone();
        all_social_topics.extend(state.social_expansion_topics.iter().cloned());

        let mut all_events = Events::new();

        if all_social_topics.is_empty() {
            all_events.push(ScrapeEvent::TopicDiscoveryCompleted {
                run_id,
                source_signal_counts: Default::default(),
                collected_links: Default::default(),
                expansion_queries: Default::default(),
                stats_delta: Default::default(),
                extracted_batches: Default::default(),
            });
        } else {
            let mut topic_output = activities::topic_discovery::discover_from_topics(
                deps,
                &all_social_topics,
                &state.url_to_canonical_key,
                &state.actor_contexts,
            ).await;

            let events = topic_output.take_events();
            all_events.extend(events);
            all_events.push(ScrapeEvent::TopicDiscoveryCompleted {
                run_id,
                source_signal_counts: topic_output.source_signal_counts,
                collected_links: topic_output.collected_links,
                expansion_queries: topic_output.expansion_queries,
                stats_delta: topic_output.stats_delta,
                extracted_batches: topic_output.extracted_batches,
            });
        }

        Ok(all_events)
    }

    // -----------------------------------------------------------------------
    // Response URL resolution — separate because it reloads sources from graph
    // -----------------------------------------------------------------------

    /// SourceExpansionCompleted or SourceExpansionSkipped → resolve response URLs.
    #[reactor(on = DiscoveryEvent, id = "scrape:resolve_new_source", filter = is_source_expansion_done, describe = describe_resolve_new_source_gate)]
    async fn resolve_new_source(
        _event: DiscoveryEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        info!("=== Phase B: Find Responses ===");
        let deps = ctx.deps();
        let state = ctx.aggregate::<PipelineState>().curr;

        let (region, graph) = match (state.run_scope.region(), deps.graph.as_ref()) {
            (Some(r), Some(g)) => (r, g),
            _ => {
                ctx.logger.debug("Skipped response scrape: missing region or graph");
                return Ok(events![ScrapeEvent::ResponseScrapeSkipped {
                    reason: "missing region or graph".into(),
                }]);
            }
        };

        let run_id = deps.run_id;
        let plan = state.source_plan.as_ref().expect("source plan stashed");

        // Reload from graph — picks up sources discovered mid-run by link promotion
        let fresh_sources = match graph
            .get_sources_for_region(region.center_lat, region.center_lng, region.radius_km)
            .await
        {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "Failed to reload sources for Phase B");
                Vec::new()
            }
        };

        // Phase B: originally-selected response + never-scraped fresh discovery
        let phase_b_sources: Vec<rootsignal_common::SourceNode> = fresh_sources
            .iter()
            .filter(|s| {
                plan.response_phase_keys.contains(&s.canonical_key)
                    || (s.last_scraped.is_none() && !plan.selected_keys.contains(&s.canonical_key))
            })
            .cloned()
            .collect();

        let mut fresh_url_mappings = std::collections::HashMap::new();
        for s in &fresh_sources {
            if let Some(ref url) = s.url {
                let clean = crate::infra::util::sanitize_url(url);
                if !state.url_to_canonical_key.contains_key(&clean) {
                    fresh_url_mappings.insert(clean, s.canonical_key.clone());
                }
            }
        }

        let mut all_events = Events::new();

        let web_sources: Vec<&rootsignal_common::SourceNode> = phase_b_sources
            .iter()
            .filter(|s| !matches!(
                rootsignal_common::scraping_strategy(s.value()),
                rootsignal_common::ScrapingStrategy::Social(_)
            ))
            .collect();

        if !web_sources.is_empty() {
            info!(count = web_sources.len(), "Phase B sources (response + fresh discovery)");
            let mut resolution = activities::url_resolution::resolve_web_urls(
                deps,
                &web_sources,
                &state.url_to_canonical_key,
            ).await;

            let web_source_nodes: Vec<rootsignal_common::SourceNode> = phase_b_sources
                .iter()
                .filter(|s| !matches!(
                    rootsignal_common::scraping_strategy(s.value()),
                    rootsignal_common::ScrapingStrategy::Social(_)
                ))
                .cloned()
                .collect();
            let web_source_keys = build_source_keys(&web_source_nodes);

            resolution.url_mappings.extend(fresh_url_mappings);

            all_events.push(ScrapeEvent::SourcesResolved {
                run_id,
                is_response_phase: true,
                web_urls: resolution.urls,
                web_source_keys,
                web_source_count: resolution.source_count,
                url_mappings: resolution.url_mappings,
                pub_dates: resolution.pub_dates,
                query_api_errors: resolution.query_api_errors,
            });
        } else {
            all_events.push(ScrapeEvent::SourcesResolved {
                run_id,
                is_response_phase: true,
                web_urls: Vec::new(),
                web_source_keys: HashMap::new(),
                web_source_count: 0,
                url_mappings: fresh_url_mappings,
                pub_dates: Default::default(),
                query_api_errors: Default::default(),
            });
        }

        Ok(all_events)
    }
}
