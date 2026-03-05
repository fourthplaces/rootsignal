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

fn is_url_fetch_requested(e: &ScrapeEvent) -> bool {
    matches!(e, ScrapeEvent::UrlFetchRequested { .. })
}

fn is_social_scrape_triggered(e: &ScrapeEvent) -> bool {
    matches!(e, ScrapeEvent::SocialScrapeTriggered { .. })
}

fn is_social_source_requested(e: &ScrapeEvent) -> bool {
    matches!(e, ScrapeEvent::SocialSourceRequested { .. })
}

fn is_topic_discovery_triggered(e: &ScrapeEvent) -> bool {
    matches!(e, ScrapeEvent::TopicDiscoveryTriggered { .. })
}

fn is_url_scrape_completed(e: &ScrapeEvent) -> bool {
    matches!(e, ScrapeEvent::UrlScrapeCompleted { .. })
}

fn is_social_source_completed(e: &ScrapeEvent) -> bool {
    matches!(e, ScrapeEvent::SocialSourceCompleted { .. })
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
            deps.ai.as_deref(),
            deps.run_scope.region().map(|r| r.name.as_str()),
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

    /// WebUrlsResolved → fan out individual UrlFetchRequested per URL.
    #[handle(on = ScrapeEvent, id = "scrape:fan_out_urls", filter = is_web_urls_resolved)]
    async fn fan_out_urls(
        event: ScrapeEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let (run_id, role, urls, source_keys) = match event {
            ScrapeEvent::WebUrlsResolved { run_id, role, urls, source_keys, .. } => (run_id, role, urls, source_keys),
            _ => unreachable!("filter guarantees WebUrlsResolved"),
        };

        info!(?role, url_count = urls.len(), "Fan-out URLs for web scrape role");

        if urls.is_empty() {
            return Ok(events![ScrapeEvent::ScrapeRoleCompleted {
                run_id,
                role,
                urls_scraped: 0,
                urls_unchanged: 0,
                urls_failed: 0,
                signals_extracted: 0,
            }]);
        }

        let (_, state) = ctx.singleton::<PipelineState>();

        let mut all_events = Events::new();
        for url in urls {
            let clean_url = crate::infra::util::sanitize_url(&url);
            let ck = state.url_to_canonical_key
                .get(&clean_url)
                .cloned()
                .unwrap_or_else(|| clean_url.clone());
            let source_id = source_keys.get(&ck).copied();
            all_events.push(ScrapeEvent::UrlFetchRequested {
                run_id,
                role,
                url,
                canonical_key: ck,
                source_id,
            });
        }

        Ok(all_events)
    }

    /// UrlFetchRequested → fetch + extract signals for a single URL.
    #[handle(on = ScrapeEvent, id = "scrape:fetch_single_url", filter = is_url_fetch_requested)]
    async fn fetch_single_url(
        event: ScrapeEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let (run_id, role, url, _canonical_key, source_id) = match event {
            ScrapeEvent::UrlFetchRequested { run_id, role, url, canonical_key, source_id } => {
                (run_id, role, url, canonical_key, source_id)
            }
            _ => unreachable!("filter guarantees UrlFetchRequested"),
        };

        let deps = ctx.deps();
        let scraper = Scraper::new(
            deps.store.clone(),
            deps.extractor.as_ref().expect("extractor set").clone(),
            deps.fetcher.as_ref().expect("fetcher set").clone(),
        );

        let (_, state) = ctx.singleton::<PipelineState>();

        let single = scraper.fetch_and_extract_single(
            &url,
            source_id,
            &state.url_to_canonical_key,
            &state.actor_contexts,
            &state.url_to_pub_date,
        ).await;

        let mut all_events = Events::new();
        all_events.push(PipelineEvent::ScrapeResultAccumulated {
            source_signal_counts: single.source_signal_counts,
            collected_links: single.collected_links,
            expansion_queries: single.expansion_queries,
            stats_delta: StatsDelta::default(),
        });
        all_events.extend(single.events);
        all_events.push(ScrapeEvent::UrlScrapeCompleted {
            run_id,
            role,
            url,
            scraped: single.scraped,
            unchanged: single.unchanged,
            failed: single.failed,
            signals_extracted: single.signals_extracted,
        });

        Ok(all_events)
    }

    /// SocialScrapeTriggered → fan out individual SocialSourceRequested per source.
    #[handle(on = ScrapeEvent, id = "scrape:fan_out_social", filter = is_social_scrape_triggered)]
    async fn fan_out_social(
        event: ScrapeEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let (run_id, role) = match &event {
            ScrapeEvent::SocialScrapeTriggered { run_id, role } => (*run_id, *role),
            _ => unreachable!("filter guarantees SocialScrapeTriggered"),
        };

        info!(?role, "Fan-out social sources for scrape role");

        let (_, state) = ctx.singleton::<PipelineState>();
        let scheduled = state.scheduled.as_ref().expect("scheduled data stashed");

        let social_sources: Vec<&rootsignal_common::SourceNode> = if matches!(role, ScrapeRole::TensionSocial) {
            scheduled.scheduled_sources
                .iter()
                .filter(|s| {
                    matches!(
                        rootsignal_common::scraping_strategy(s.value()),
                        rootsignal_common::ScrapingStrategy::Social(_)
                    ) && scheduled.tension_phase_keys.contains(&s.canonical_key)
                })
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
            }]);
        }

        let mut all_events = Events::new();
        for source in social_sources {
            let common_platform = match rootsignal_common::scraping_strategy(source.value()) {
                rootsignal_common::ScrapingStrategy::Social(p) => p,
                _ => continue,
            };
            let (platform_str, identifier) = match common_platform {
                rootsignal_common::SocialPlatform::Instagram => (
                    "instagram".to_string(),
                    source.url.as_deref().unwrap_or(&source.canonical_value).to_string(),
                ),
                rootsignal_common::SocialPlatform::Facebook => {
                    let url = source.url.as_deref().filter(|u| !u.is_empty()).unwrap_or(&source.canonical_value);
                    ("facebook".to_string(), url.to_string())
                }
                rootsignal_common::SocialPlatform::Reddit => {
                    let url = source.url.as_deref().filter(|u| !u.is_empty()).unwrap_or(&source.canonical_value);
                    let identifier = if !url.starts_with("http") {
                        let name = url.trim_start_matches("r/");
                        format!("https://www.reddit.com/r/{}/", name)
                    } else {
                        url.to_string()
                    };
                    ("reddit".to_string(), identifier)
                }
                rootsignal_common::SocialPlatform::Twitter => (
                    "twitter".to_string(),
                    source.url.as_deref().unwrap_or(&source.canonical_value).to_string(),
                ),
                rootsignal_common::SocialPlatform::TikTok => (
                    "tiktok".to_string(),
                    source.url.as_deref().unwrap_or(&source.canonical_value).to_string(),
                ),
                rootsignal_common::SocialPlatform::Bluesky => continue,
            };
            let source_url = source.url.as_deref().filter(|u| !u.is_empty()).unwrap_or(&source.canonical_value).to_string();
            all_events.push(ScrapeEvent::SocialSourceRequested {
                run_id,
                role,
                canonical_key: source.canonical_key.clone(),
                source_url,
                platform: platform_str,
                identifier,
            });
        }

        // If all sources were skipped (e.g. all Bluesky), complete immediately
        if all_events.is_empty() {
            return Ok(events![ScrapeEvent::ScrapeRoleCompleted {
                run_id,
                role,
                urls_scraped: 0,
                urls_unchanged: 0,
                urls_failed: 0,
                signals_extracted: 0,
            }]);
        }

        Ok(all_events)
    }

    /// SocialSourceRequested → fetch + extract signals for a single social source.
    #[handle(on = ScrapeEvent, id = "scrape:fetch_single_social", filter = is_social_source_requested)]
    async fn fetch_single_social(
        event: ScrapeEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let (run_id, role, canonical_key, source_url, platform_str, identifier) = match event {
            ScrapeEvent::SocialSourceRequested {
                run_id, role, canonical_key, source_url, platform, identifier,
            } => (run_id, role, canonical_key, source_url, platform, identifier),
            _ => unreachable!("filter guarantees SocialSourceRequested"),
        };

        let platform = match platform_str.as_str() {
            "instagram" => rootsignal_common::SocialPlatform::Instagram,
            "facebook" => rootsignal_common::SocialPlatform::Facebook,
            "reddit" => rootsignal_common::SocialPlatform::Reddit,
            "twitter" => rootsignal_common::SocialPlatform::Twitter,
            "tiktok" => rootsignal_common::SocialPlatform::TikTok,
            _ => rootsignal_common::SocialPlatform::Instagram, // fallback
        };

        let deps = ctx.deps();
        let scraper = Scraper::new(
            deps.store.clone(),
            deps.extractor.as_ref().expect("extractor set").clone(),
            deps.fetcher.as_ref().expect("fetcher set").clone(),
        );

        let (_, state) = ctx.singleton::<PipelineState>();

        // Look up source_id from scheduled data
        let source_id = state.scheduled.as_ref().and_then(|sched| {
            sched.scheduled_sources
                .iter()
                .find(|s| s.canonical_key == canonical_key)
                .map(|s| s.id)
        });

        let single = scraper.scrape_single_social_source(
            &canonical_key,
            &source_url,
            platform,
            &identifier,
            source_id,
            &state.url_to_canonical_key,
            &state.actor_contexts,
        ).await;

        let mut all_events = Events::new();
        all_events.push(PipelineEvent::ScrapeResultAccumulated {
            source_signal_counts: single.source_signal_counts,
            collected_links: single.collected_links,
            expansion_queries: single.expansion_queries,
            stats_delta: single.stats_delta,
        });
        all_events.extend(single.events);
        all_events.push(ScrapeEvent::SocialSourceCompleted {
            run_id,
            role,
            canonical_key,
            posts_fetched: single.posts_fetched,
            signals_extracted: single.signals_extracted,
        });

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

    /// UrlScrapeCompleted → check if all URLs for role are done, emit ScrapeRoleCompleted.
    #[handle(on = ScrapeEvent, id = "scrape:check_url_role_complete", filter = is_url_scrape_completed)]
    async fn check_url_role_complete(
        event: ScrapeEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let (run_id, role) = match &event {
            ScrapeEvent::UrlScrapeCompleted { run_id, role, .. } => (*run_id, *role),
            _ => unreachable!("filter guarantees UrlScrapeCompleted"),
        };

        let (_, state) = ctx.singleton::<PipelineState>();

        let total = state.role_url_totals.get(&role).copied().unwrap_or(0);
        let completed = state.role_urls_completed.get(&role).copied().unwrap_or(0);

        if total > 0 && completed >= total {
            let stats = state.role_stats.get(&role).cloned().unwrap_or_default();
            info!(?role, total, completed, "All URLs complete, emitting ScrapeRoleCompleted");
            Ok(events![ScrapeEvent::ScrapeRoleCompleted {
                run_id,
                role,
                urls_scraped: stats.urls_scraped,
                urls_unchanged: stats.urls_unchanged,
                urls_failed: stats.urls_failed,
                signals_extracted: stats.signals_extracted,
            }])
        } else {
            Ok(events![PipelineEvent::HandlerSkipped {
                handler_id: "scrape:check_url_role_complete".into(),
                reason: format!("waiting for {role:?}: {completed}/{total} URLs complete"),
            }])
        }
    }

    /// SocialSourceCompleted → check if all social sources for role are done, emit ScrapeRoleCompleted.
    #[handle(on = ScrapeEvent, id = "scrape:check_social_role_complete", filter = is_social_source_completed)]
    async fn check_social_role_complete(
        event: ScrapeEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let (run_id, role) = match &event {
            ScrapeEvent::SocialSourceCompleted { run_id, role, .. } => (*run_id, *role),
            _ => unreachable!("filter guarantees SocialSourceCompleted"),
        };

        let (_, state) = ctx.singleton::<PipelineState>();

        let total = state.role_url_totals.get(&role).copied().unwrap_or(0);
        let completed = state.role_urls_completed.get(&role).copied().unwrap_or(0);

        if total > 0 && completed >= total {
            let stats = state.role_stats.get(&role).cloned().unwrap_or_default();
            info!(?role, total, completed, "All social sources complete, emitting ScrapeRoleCompleted");
            Ok(events![ScrapeEvent::ScrapeRoleCompleted {
                run_id,
                role,
                urls_scraped: stats.urls_scraped,
                urls_unchanged: stats.urls_unchanged,
                urls_failed: stats.urls_failed,
                signals_extracted: stats.signals_extracted,
            }])
        } else {
            Ok(events![PipelineEvent::HandlerSkipped {
                handler_id: "scrape:check_social_role_complete".into(),
                reason: format!("waiting for {role:?}: {completed}/{total} social sources complete"),
            }])
        }
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

        // Requires region + graph_client — skip in tests
        let (region, graph_client) = match (deps.run_scope.region(), deps.graph_client.as_ref()) {
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
                            "region": deps.run_scope.region().is_none(),
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
                deps.ai.as_deref(),
                deps.run_scope.region().map(|r| r.name.as_str()),
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
