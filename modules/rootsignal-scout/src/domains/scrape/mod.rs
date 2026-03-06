// Scrape domain: tension and response scrape phase handlers.

pub mod activities;
pub mod events;

#[cfg(test)]
mod boundary_tests;
#[cfg(test)]
mod chain_tests;
#[cfg(test)]
pub mod simweb_adapter;

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};
use tracing::info;
use uuid::Uuid;



use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::core::events::PipelinePhase;
use crate::core::pipeline_events::PipelineEvent;
use crate::domains::discovery::events::DiscoveryEvent;
use crate::domains::lifecycle::events::LifecycleEvent;
use crate::domains::scrape::activities::StatsDelta;
use rootsignal_common::telemetry_events::TelemetryEvent;

use crate::domains::scrape::events::{ScrapeEvent, ScrapeRole};

fn is_sources_prepared(e: &LifecycleEvent) -> bool {
    matches!(e, LifecycleEvent::SourcesPrepared { .. })
}

fn is_source_expansion_completed(e: &LifecycleEvent) -> bool {
    matches!(
        e,
        LifecycleEvent::PhaseCompleted { phase }
            if matches!(phase, PipelinePhase::SourceExpansion)
    )
}

fn is_sources_resolved(e: &ScrapeEvent) -> bool {
    matches!(e, ScrapeEvent::SourcesResolved { .. })
}

fn is_response_sources_resolved(e: &ScrapeEvent) -> bool {
    matches!(e, ScrapeEvent::SourcesResolved { web_role: ScrapeRole::ResponseWeb, .. })
}

fn is_scrape_role_completed(e: &ScrapeEvent) -> bool {
    matches!(e, ScrapeEvent::ScrapeRoleCompleted { .. })
}

/// Expected roles for each scrape phase, used for completion tracking.
fn tension_roles() -> HashSet<ScrapeRole> {
    HashSet::from([ScrapeRole::TensionWeb, ScrapeRole::TensionSocial])
}

fn response_roles() -> HashSet<ScrapeRole> {
    HashSet::from([ScrapeRole::ResponseWeb, ScrapeRole::ResponseSocial, ScrapeRole::TopicDiscovery])
}

/// Build source_keys (canonical_key → source_id) from filtered sources.
fn build_source_keys(sources: &[rootsignal_common::SourceNode]) -> HashMap<String, Uuid> {
    sources.iter().map(|s| (s.canonical_key.clone(), s.id)).collect()
}

#[handlers]
pub mod handlers {
    use super::*;

    /// SourcesPrepared → resolve tension URLs, emit SourcesResolved.
    #[handle(on = LifecycleEvent, id = "scrape:resolve_tension", filter = is_sources_prepared)]
    async fn resolve_tension(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        info!("=== Phase A: Find Problems ===");
        let deps = ctx.deps();

        let (_, state) = ctx.singleton::<PipelineState>();
        let run_id = Uuid::parse_str(&deps.run_id).unwrap_or_else(|_| Uuid::new_v4());

        let plan = state.source_plan.as_ref().expect("source plan stashed");

        // Resolve tension web sources
        let tension_web: Vec<rootsignal_common::SourceNode> = plan
            .selected_sources
            .iter()
            .filter(|s| {
                plan.tension_phase_keys.contains(&s.canonical_key)
                    && !matches!(
                        rootsignal_common::scraping_strategy(s.value()),
                        rootsignal_common::ScrapingStrategy::Social(_)
                    )
            })
            .cloned()
            .collect();

        let tension_web_refs: Vec<&rootsignal_common::SourceNode> = tension_web.iter().collect();
        let resolution = activities::url_resolution::resolve_web_urls(
            deps,
            &tension_web_refs,
            &state.url_to_canonical_key,
        ).await;

        let tension_web_keys = build_source_keys(&tension_web);

        Ok(events![ScrapeEvent::SourcesResolved {
            run_id,
            web_role: ScrapeRole::TensionWeb,
            web_urls: resolution.urls,
            web_source_keys: tension_web_keys,
            web_source_count: resolution.source_count,
            url_mappings: resolution.url_mappings,
            pub_dates: resolution.pub_dates,
            query_api_errors: resolution.query_api_errors,
        }])
    }

    /// SourcesResolved → fetch + extract all URLs in batch, emit ScrapeRoleCompleted.
    #[handle(on = ScrapeEvent, id = "scrape:scrape_web", filter = is_sources_resolved)]
    async fn scrape_web(
        event: ScrapeEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let (run_id, role, urls, source_keys) = match event {
            ScrapeEvent::SourcesResolved { run_id, web_role, web_urls, web_source_keys, .. } => (run_id, web_role, web_urls, web_source_keys),
            _ => unreachable!("filter guarantees SourcesResolved"),
        };

        info!(?role, url_count = urls.len(), "Scraping web URLs for role");

        if urls.is_empty() {
            return Ok(events![ScrapeEvent::ScrapeRoleCompleted {
                run_id,
                role,
                urls_scraped: 0,
                urls_unchanged: 0,
                urls_failed: 0,
                signals_extracted: 0,
                source_signal_counts: Default::default(),
                collected_links: Default::default(),
                expansion_queries: Default::default(),
                stats_delta: Default::default(),
                page_previews: Default::default(),
                extracted_batches: Default::default(),
                discovered_sources: Default::default(),
            }]);
        }

        let deps = ctx.deps();
        let (_, state) = ctx.singleton::<PipelineState>();

        let fetch_result = activities::web_scrape::fetch_and_extract(
            deps,
            &urls,
            &source_keys,
            &state.url_to_canonical_key,
            &state.actor_contexts,
            &state.url_to_pub_date,
        ).await;

        let mut all_events = Events::new();
        all_events.extend(fetch_result.events);
        all_events.push(ScrapeEvent::ScrapeRoleCompleted {
            run_id,
            role,
            urls_scraped: fetch_result.stats.urls_scraped,
            urls_unchanged: fetch_result.stats.urls_unchanged,
            urls_failed: fetch_result.stats.urls_failed,
            signals_extracted: fetch_result.stats.signals_extracted,
            source_signal_counts: fetch_result.source_signal_counts,
            collected_links: fetch_result.collected_links,
            expansion_queries: fetch_result.expansion_queries,
            stats_delta: StatsDelta::default(),
            page_previews: fetch_result.page_previews,
            extracted_batches: fetch_result.extracted_batches,
            discovered_sources: Vec::new(),
        });

        Ok(all_events)
    }

    /// SourcesResolved → scrape all social sources in batch, emit ScrapeRoleCompleted.
    #[handle(on = ScrapeEvent, id = "scrape:scrape_social", filter = is_sources_resolved)]
    async fn scrape_social(
        event: ScrapeEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let (run_id, web_role) = match &event {
            ScrapeEvent::SourcesResolved { run_id, web_role, .. } => (*run_id, *web_role),
            _ => unreachable!("filter guarantees SourcesResolved"),
        };

        // Derive social role from web role
        let role = match web_role {
            ScrapeRole::TensionWeb => ScrapeRole::TensionSocial,
            ScrapeRole::ResponseWeb => ScrapeRole::ResponseSocial,
            _ => unreachable!("SourcesResolved always has TensionWeb or ResponseWeb"),
        };

        info!(?role, "Scraping social sources for role");

        let deps = ctx.deps();
        let (_, state) = ctx.singleton::<PipelineState>();
        let plan = state.source_plan.as_ref().expect("source plan stashed");

        let social_sources: Vec<&rootsignal_common::SourceNode> = if matches!(role, ScrapeRole::TensionSocial) {
            plan.selected_sources
                .iter()
                .filter(|s| {
                    matches!(
                        rootsignal_common::scraping_strategy(s.value()),
                        rootsignal_common::ScrapingStrategy::Social(_)
                    ) && plan.tension_phase_keys.contains(&s.canonical_key)
                })
                .collect()
        } else {
            plan.selected_sources
                .iter()
                .filter(|s| {
                    matches!(
                        rootsignal_common::scraping_strategy(s.value()),
                        rootsignal_common::ScrapingStrategy::Social(_)
                    ) && plan.response_phase_keys.contains(&s.canonical_key)
                })
                .collect()
        };

        if social_sources.is_empty() {
            return Ok(events![ScrapeEvent::ScrapeRoleCompleted {
                run_id,
                role,
                urls_scraped: 0,
                urls_unchanged: 0,
                urls_failed: 0,
                signals_extracted: 0,
                source_signal_counts: Default::default(),
                collected_links: Default::default(),
                expansion_queries: Default::default(),
                stats_delta: Default::default(),
                page_previews: Default::default(),
                extracted_batches: Default::default(),
                discovered_sources: Default::default(),
            }]);
        }

        let mut social_output = activities::social_scrape::scrape_social_sources(
            deps,
            &social_sources,
            &state.url_to_canonical_key,
            &state.actor_contexts,
        ).await;

        let events = social_output.take_events();
        let signals_extracted: u32 = social_output.source_signal_counts.values().sum();

        let mut all_events = Events::new();
        all_events.extend(events);
        all_events.push(ScrapeEvent::ScrapeRoleCompleted {
            run_id,
            role,
            urls_scraped: social_sources.len() as u32,
            urls_unchanged: 0,
            urls_failed: 0,
            signals_extracted,
            source_signal_counts: social_output.source_signal_counts,
            collected_links: social_output.collected_links,
            expansion_queries: social_output.expansion_queries,
            stats_delta: social_output.stats_delta,
            page_previews: Default::default(),
            extracted_batches: social_output.extracted_batches,
            discovered_sources: Vec::new(),
        });

        Ok(all_events)
    }

    /// SourcesResolved(ResponseWeb) → discover from social topics.
    #[handle(on = ScrapeEvent, id = "scrape:fetch_topics", filter = is_response_sources_resolved)]
    async fn fetch_topics(
        event: ScrapeEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let run_id = match &event {
            ScrapeEvent::SourcesResolved { run_id, .. } => *run_id,
            _ => unreachable!("filter guarantees SourcesResolved(ResponseWeb)"),
        };

        info!("Fetch topics for topic discovery");

        let deps = ctx.deps();

        let (_, state) = ctx.singleton::<PipelineState>();

        let mut all_social_topics = state.social_topics.clone();
        all_social_topics.extend(state.social_expansion_topics.iter().cloned());

        let mut all_events = Events::new();

        if all_social_topics.is_empty() {
            all_events.push(ScrapeEvent::ScrapeRoleCompleted {
                run_id,
                role: ScrapeRole::TopicDiscovery,
                urls_scraped: 0,
                urls_unchanged: 0,
                urls_failed: 0,
                signals_extracted: 0,
                source_signal_counts: Default::default(),
                collected_links: Default::default(),
                expansion_queries: Default::default(),
                stats_delta: Default::default(),
                page_previews: Default::default(),
                extracted_batches: Default::default(),
                discovered_sources: Default::default(),
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
            all_events.push(ScrapeEvent::ScrapeRoleCompleted {
                run_id,
                role: ScrapeRole::TopicDiscovery,
                urls_scraped: 0,
                urls_unchanged: 0,
                urls_failed: 0,
                signals_extracted: 0,
                source_signal_counts: topic_output.source_signal_counts,
                collected_links: topic_output.collected_links,
                expansion_queries: topic_output.expansion_queries,
                stats_delta: topic_output.stats_delta,
                page_previews: Default::default(),
                extracted_batches: topic_output.extracted_batches,
                discovered_sources: topic_output.discovered_sources,
            });
        }

        Ok(all_events)
    }

    /// ScrapeRoleCompleted → check if all roles for current phase are done, emit PhaseCompleted.
    #[handle(on = ScrapeEvent, id = "scrape:phase_complete", filter = is_scrape_role_completed)]
    async fn phase_complete(
        event: ScrapeEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let role = match &event {
            ScrapeEvent::ScrapeRoleCompleted { role, .. } => *role,
            _ => unreachable!("filter guarantees ScrapeRoleCompleted"),
        };

        let (_, state) = ctx.singleton::<PipelineState>();

        // Determine which phase this role belongs to and its expected roles
        let (phase, expected_roles) = match role {
            ScrapeRole::TensionWeb | ScrapeRole::TensionSocial => {
                (PipelinePhase::TensionScrape, tension_roles())
            }
            ScrapeRole::ResponseWeb | ScrapeRole::ResponseSocial | ScrapeRole::TopicDiscovery => {
                (PipelinePhase::ResponseScrape, response_roles())
            }
        };

        // Idempotency: if this phase already completed, skip
        if state.completed_phases.contains(&phase) {
            return Ok(events![PipelineEvent::HandlerSkipped {
                handler_id: "scrape:phase_complete".into(),
                reason: format!("{phase:?} already completed"),
            }]);
        }

        // Check if all expected roles are complete (including this one, which was just applied)
        if state.completed_scrape_roles.is_superset(&expected_roles) {
            info!(?phase, "All scrape roles complete, emitting PhaseCompleted");
            let mut out = events![LifecycleEvent::PhaseCompleted { phase }];
            // Emit SourcesDiscovered from stashed sources at phase boundary
            if !state.scrape_discovered_sources.is_empty() {
                out = out.add(DiscoveryEvent::SourcesDiscovered {
                    sources: state.scrape_discovered_sources.clone(),
                    discovered_by: "topic_discovery".into(),
                });
            }
            Ok(out)
        } else {
            let completed: Vec<_> = state.completed_scrape_roles.iter().collect();
            let expected: Vec<_> = expected_roles.iter().collect();
            Ok(events![PipelineEvent::HandlerSkipped {
                handler_id: "scrape:phase_complete".into(),
                reason: format!("waiting for {phase:?}: completed {completed:?}, need {expected:?}"),
            }])
        }
    }

    /// PhaseCompleted(SourceExpansion) → resolve response URLs, emit per-role events.
    #[handle(on = LifecycleEvent, id = "scrape:resolve_response", filter = is_source_expansion_completed)]
    async fn resolve_response(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        info!("=== Phase B: Find Responses ===");
        let deps = ctx.deps();

        // Requires region + graph — skip in tests
        let (region, graph) = match (deps.run_scope.region(), deps.graph.as_ref()) {
            (Some(r), Some(g)) => (r, g),
            _ => {
                ctx.logger.debug("Skipped response scrape resolve: missing region or graph");
                return Ok(events![LifecycleEvent::PhaseCompleted {
                    phase: PipelinePhase::ResponseScrape,
                }]);
            }
        };

        let (_, state) = ctx.singleton::<PipelineState>();
        let run_id = Uuid::parse_str(&deps.run_id).unwrap_or_else(|_| Uuid::new_v4());
        let plan = state.source_plan.as_ref().expect("source plan stashed");

        // Reload sources from graph to pick up mid-run discoveries
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

        // Build fresh URL mappings
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

        // Resolve web URLs for response phase
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

            // Build source_keys from web sources for fetch handler
            let web_source_nodes: Vec<rootsignal_common::SourceNode> = phase_b_sources
                .iter()
                .filter(|s| !matches!(
                    rootsignal_common::scraping_strategy(s.value()),
                    rootsignal_common::ScrapingStrategy::Social(_)
                ))
                .cloned()
                .collect();
            let web_source_keys = build_source_keys(&web_source_nodes);

            // Merge fresh URL mappings into resolution so SourcesResolved carries everything
            resolution.url_mappings.extend(fresh_url_mappings);

            all_events.push(ScrapeEvent::SourcesResolved {
                run_id,
                web_role: ScrapeRole::ResponseWeb,
                web_urls: resolution.urls,
                web_source_keys,
                web_source_count: resolution.source_count,
                url_mappings: resolution.url_mappings,
                pub_dates: resolution.pub_dates,
                query_api_errors: resolution.query_api_errors,
            });
        } else {
            // No web sources — emit empty SourcesResolved to trigger completion
            // Include fresh URL mappings so they're applied to state
            all_events.push(ScrapeEvent::SourcesResolved {
                run_id,
                web_role: ScrapeRole::ResponseWeb,
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
