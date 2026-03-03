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

use rootsignal_graph::GraphReader;

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::core::events::PipelinePhase;
use crate::core::pipeline_events::PipelineEvent;
use crate::domains::lifecycle::events::LifecycleEvent;
use crate::domains::scrape::activities::Scraper;
use crate::domains::scrape::activities::StatsDelta;
use rootsignal_common::telemetry_events::TelemetryEvent;

use crate::domains::scrape::events::{ScrapeEvent, ScrapeRole};

fn is_sources_scheduled(e: &LifecycleEvent) -> bool {
    matches!(e, LifecycleEvent::SourcesScheduled { .. })
}

fn is_source_expansion_completed(e: &LifecycleEvent) -> bool {
    matches!(
        e,
        LifecycleEvent::PhaseCompleted { phase }
            if matches!(phase, PipelinePhase::SourceExpansion)
    )
}

fn is_web_urls_resolved(e: &ScrapeEvent) -> bool {
    matches!(e, ScrapeEvent::WebUrlsResolved { .. })
}

fn is_social_scrape_triggered(e: &ScrapeEvent) -> bool {
    matches!(e, ScrapeEvent::SocialScrapeTriggered { .. })
}

fn is_topic_discovery_triggered(e: &ScrapeEvent) -> bool {
    matches!(e, ScrapeEvent::TopicDiscoveryTriggered { .. })
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

    /// SourcesScheduled → resolve tension URLs, emit WebUrlsResolved + SocialScrapeTriggered.
    #[handle(on = LifecycleEvent, id = "scrape:resolve_tension", filter = is_sources_scheduled)]
    async fn resolve_tension(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        info!("=== Phase A: Find Problems ===");
        let deps = ctx.deps();

        let phase = Scraper::new(
            deps.store.clone(),
            deps.extractor.as_ref().expect("extractor set").clone(),
            deps.fetcher.as_ref().expect("fetcher set").clone(),
        );

        let (_, state) = ctx.singleton::<PipelineState>();
        let run_id = Uuid::parse_str(&deps.run_id).unwrap_or_else(|_| Uuid::new_v4());

        let scheduled = state.scheduled.as_ref().expect("scheduled data stashed");

        // Resolve tension web sources
        let tension_web: Vec<rootsignal_common::SourceNode> = scheduled
            .scheduled_sources
            .iter()
            .filter(|s| {
                scheduled.tension_phase_keys.contains(&s.canonical_key)
                    && !matches!(
                        rootsignal_common::scraping_strategy(s.value()),
                        rootsignal_common::ScrapingStrategy::Social(_)
                    )
            })
            .cloned()
            .collect();

        let tension_web_refs: Vec<&rootsignal_common::SourceNode> = tension_web.iter().collect();
        let resolution = phase.resolve_web_urls(
            &tension_web_refs,
            &state.url_to_canonical_key,
        ).await;

        let mut all_events = Events::new();

        // Emit UrlsResolvedAccumulated for URL mappings + pub_dates
        all_events.push(PipelineEvent::UrlsResolvedAccumulated {
            url_mappings: resolution.url_mappings.clone(),
            pub_dates: resolution.pub_dates.clone(),
            query_api_errors: resolution.query_api_errors.clone(),
        });

        // Emit WebUrlsResolved for web sources — carry source_keys so fetch handler doesn't re-derive
        let tension_web_keys = build_source_keys(&tension_web);
        all_events.push(ScrapeEvent::WebUrlsResolved {
            run_id,
            role: ScrapeRole::TensionWeb,
            urls: resolution.urls,
            source_keys: tension_web_keys,
            source_count: resolution.source_count,
        });

        // Trigger social scrape — handler reads sources from scheduled state
        all_events.push(ScrapeEvent::SocialScrapeTriggered {
            run_id,
            role: ScrapeRole::TensionSocial,
        });

        Ok(all_events)
    }

    /// WebUrlsResolved → fetch + extract signals for web URLs.
    #[handle(on = ScrapeEvent, id = "scrape:fetch_extract", filter = is_web_urls_resolved)]
    async fn fetch_extract(
        event: ScrapeEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let (run_id, role, urls, source_keys) = match event {
            ScrapeEvent::WebUrlsResolved { run_id, role, urls, source_keys, .. } => (run_id, role, urls, source_keys),
            _ => unreachable!("filter guarantees WebUrlsResolved"),
        };

        info!(?role, url_count = urls.len(), "Fetch+extract for web scrape role");

        let deps = ctx.deps();
        let phase = Scraper::new(
            deps.store.clone(),
            deps.extractor.as_ref().expect("extractor set").clone(),
            deps.fetcher.as_ref().expect("fetcher set").clone(),
        );

        let (_, state) = ctx.singleton::<PipelineState>();

        let fetch_result = phase.fetch_and_extract(
            &urls,
            &source_keys,
            &state.url_to_canonical_key,
            &state.actor_contexts,
            &state.url_to_pub_date,
        ).await;

        let mut all_events = Events::new();
        all_events.push(PipelineEvent::ScrapeResultAccumulated {
            source_signal_counts: fetch_result.source_signal_counts,
            collected_links: fetch_result.collected_links,
            expansion_queries: fetch_result.expansion_queries,
            stats_delta: StatsDelta::default(),
        });
        all_events.extend(fetch_result.events);
        all_events.push(ScrapeEvent::ScrapeRoleCompleted {
            run_id,
            role,
            urls_scraped: fetch_result.stats.urls_scraped,
            urls_unchanged: fetch_result.stats.urls_unchanged,
            urls_failed: fetch_result.stats.urls_failed,
            signals_extracted: fetch_result.stats.signals_extracted,
        });

        Ok(all_events)
    }

    /// SocialScrapeTriggered → scrape social sources.
    #[handle(on = ScrapeEvent, id = "scrape:fetch_social", filter = is_social_scrape_triggered)]
    async fn fetch_social(
        event: ScrapeEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let (run_id, role) = match &event {
            ScrapeEvent::SocialScrapeTriggered { run_id, role } => (*run_id, *role),
            _ => unreachable!("filter guarantees SocialScrapeTriggered"),
        };

        info!(?role, "Fetch social sources for scrape role");

        let deps = ctx.deps();
        let phase = Scraper::new(
            deps.store.clone(),
            deps.extractor.as_ref().expect("extractor set").clone(),
            deps.fetcher.as_ref().expect("fetcher set").clone(),
        );

        let (_, state) = ctx.singleton::<PipelineState>();
        let scheduled = state.scheduled.as_ref().expect("scheduled data stashed");

        let social_sources: Vec<rootsignal_common::SourceNode> = if matches!(role, ScrapeRole::TensionSocial) {
            scheduled.scheduled_sources
                .iter()
                .filter(|s| {
                    matches!(
                        rootsignal_common::scraping_strategy(s.value()),
                        rootsignal_common::ScrapingStrategy::Social(_)
                    ) && scheduled.tension_phase_keys.contains(&s.canonical_key)
                })
                .cloned()
                .collect()
        } else {
            scheduled.scheduled_sources
                .iter()
                .filter(|s| {
                    matches!(
                        rootsignal_common::scraping_strategy(s.value()),
                        rootsignal_common::ScrapingStrategy::Social(_)
                    ) && scheduled.response_phase_keys.contains(&s.canonical_key)
                })
                .cloned()
                .collect()
        };

        let mut all_events = Events::new();

        if social_sources.is_empty() {
            all_events.push(ScrapeEvent::ScrapeRoleCompleted {
                run_id,
                role,
                urls_scraped: 0,
                urls_unchanged: 0,
                urls_failed: 0,
                signals_extracted: 0,
            });
        } else {
            let social_refs: Vec<&rootsignal_common::SourceNode> = social_sources.iter().collect();
            let mut social_output = phase.scrape_social_sources(
                &social_refs,
                &state.url_to_canonical_key,
                &state.actor_contexts,
            ).await;

            let events = social_output.take_events();
            all_events.push(PipelineEvent::ScrapeResultAccumulated {
                source_signal_counts: social_output.source_signal_counts,
                collected_links: social_output.collected_links,
                expansion_queries: social_output.expansion_queries,
                stats_delta: social_output.stats_delta,
            });
            all_events.extend(events);
            all_events.push(ScrapeEvent::ScrapeRoleCompleted {
                run_id,
                role,
                urls_scraped: 0,
                urls_unchanged: 0,
                urls_failed: 0,
                signals_extracted: 0,
            });
        }

        Ok(all_events)
    }

    /// TopicDiscoveryTriggered → discover from social topics.
    #[handle(on = ScrapeEvent, id = "scrape:fetch_topics", filter = is_topic_discovery_triggered)]
    async fn fetch_topics(
        event: ScrapeEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let run_id = match &event {
            ScrapeEvent::TopicDiscoveryTriggered { run_id } => *run_id,
            _ => unreachable!("filter guarantees TopicDiscoveryTriggered"),
        };

        info!("Fetch topics for topic discovery");

        let deps = ctx.deps();
        let phase = Scraper::new(
            deps.store.clone(),
            deps.extractor.as_ref().expect("extractor set").clone(),
            deps.fetcher.as_ref().expect("fetcher set").clone(),
        );

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
            });
        } else {
            all_events.push(PipelineEvent::SocialTopicsConsumed);
            let mut topic_output = phase.discover_from_topics(
                &all_social_topics,
                &state.url_to_canonical_key,
                &state.actor_contexts,
            ).await;

            let events = topic_output.take_events();
            all_events.push(PipelineEvent::ScrapeResultAccumulated {
                source_signal_counts: topic_output.source_signal_counts,
                collected_links: topic_output.collected_links,
                expansion_queries: topic_output.expansion_queries,
                stats_delta: topic_output.stats_delta,
            });
            all_events.extend(events);
            all_events.push(ScrapeEvent::ScrapeRoleCompleted {
                run_id,
                role: ScrapeRole::TopicDiscovery,
                urls_scraped: 0,
                urls_unchanged: 0,
                urls_failed: 0,
                signals_extracted: 0,
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

        // Check if all expected roles are complete (including this one, which was just applied)
        if state.completed_scrape_roles.is_superset(&expected_roles) {
            info!(?phase, "All scrape roles complete, emitting PhaseCompleted");
            Ok(events![LifecycleEvent::PhaseCompleted { phase }])
        } else {
            Ok(Events::new())
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

        // Requires region + graph_client — skip in tests
        let (region, graph_client) = match (deps.region.as_ref(), deps.graph_client.as_ref()) {
            (Some(r), Some(g)) => (r, g),
            _ => {
                let mut skip = events![LifecycleEvent::PhaseCompleted {
                    phase: PipelinePhase::ResponseScrape,
                }];
                skip.push(TelemetryEvent::SystemLog {
                    message: "Skipped response scrape resolve: missing region or graph_client".into(),
                    context: Some(serde_json::json!({
                        "handler": "scrape:resolve_response",
                        "reason": "missing_deps",
                        "missing": {
                            "region": deps.region.is_none(),
                            "graph_client": deps.graph_client.is_none(),
                        },
                    })),
                });
                return Ok(skip);
            }
        };
        let graph = GraphReader::new(graph_client.clone());

        let phase = Scraper::new(
            deps.store.clone(),
            deps.extractor.as_ref().expect("extractor set").clone(),
            deps.fetcher.as_ref().expect("fetcher set").clone(),
        );

        let (_, state) = ctx.singleton::<PipelineState>();
        let run_id = Uuid::parse_str(&deps.run_id).unwrap_or_else(|_| Uuid::new_v4());
        let scheduled = state.scheduled.as_ref().expect("scheduled data stashed");

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

        // Phase B: originally-scheduled response + never-scraped fresh discovery
        let phase_b_sources: Vec<rootsignal_common::SourceNode> = fresh_sources
            .iter()
            .filter(|s| {
                scheduled.response_phase_keys.contains(&s.canonical_key)
                    || (s.last_scraped.is_none() && !scheduled.scheduled_keys.contains(&s.canonical_key))
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

        // Emit fresh URL mappings
        if !fresh_url_mappings.is_empty() {
            all_events.push(PipelineEvent::UrlsResolvedAccumulated {
                url_mappings: fresh_url_mappings,
                pub_dates: Default::default(),
                query_api_errors: Default::default(),
            });
        }

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
            let resolution = phase.resolve_web_urls(
                &web_sources,
                &state.url_to_canonical_key,
            ).await;

            all_events.push(PipelineEvent::UrlsResolvedAccumulated {
                url_mappings: resolution.url_mappings,
                pub_dates: resolution.pub_dates,
                query_api_errors: resolution.query_api_errors,
            });

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

            all_events.push(ScrapeEvent::WebUrlsResolved {
                run_id,
                role: ScrapeRole::ResponseWeb,
                urls: resolution.urls,
                source_keys: web_source_keys,
                source_count: resolution.source_count,
            });
        } else {
            // No web sources — emit empty WebUrlsResolved to trigger completion
            all_events.push(ScrapeEvent::WebUrlsResolved {
                run_id,
                role: ScrapeRole::ResponseWeb,
                urls: Vec::new(),
                source_keys: HashMap::new(),
                source_count: 0,
            });
        }

        // Trigger social scrape — handler reads sources from scheduled state
        all_events.push(ScrapeEvent::SocialScrapeTriggered {
            run_id,
            role: ScrapeRole::ResponseSocial,
        });

        // Trigger topic discovery — handler reads topics from PipelineState
        all_events.push(ScrapeEvent::TopicDiscoveryTriggered {
            run_id,
        });

        Ok(all_events)
    }
}
