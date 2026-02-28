//! Seesaw handlers for the scrape domain: tension and response scraping.

use std::sync::Arc;

use seesaw_core::{events, on, Context, Events, Handler};
use tracing::{info, warn};

use rootsignal_common::{scraping_strategy, ScrapingStrategy, SourceNode};
use rootsignal_graph::GraphWriter;

use crate::core::engine::ScoutEngineDeps;
use crate::core::events::{PipelineEvent, PipelinePhase, ScoutEvent};
use crate::domains::lifecycle::events::LifecycleEvent;
use crate::domains::signals::events::SignalEvent;
use crate::infra::util::sanitize_url;
use crate::pipeline::scrape_phase::ScrapePhase;

use crate::infra::run_log::RunLogger;

/// Partition collected events: convert PipelineEvent::SignalsExtracted to
/// SignalEvent::SignalsExtracted for TypeId routing, keep everything else as ScoutEvent.
fn partition_into_events(collected: Vec<ScoutEvent>, tail: LifecycleEvent) -> Events {
    let mut events = Events::new();
    for e in collected {
        match e {
            ScoutEvent::Pipeline(PipelineEvent::SignalsExtracted {
                url,
                canonical_key,
                count,
                batch,
            }) => {
                events = events.add(SignalEvent::SignalsExtracted {
                    url,
                    canonical_key,
                    count,
                    batch,
                });
            }
            other => {
                events = events.add(other);
            }
        }
    }
    events.add(tail)
}

async fn make_run_log(
    run_id: &str,
    region_name: &str,
    pg_pool: Option<&sqlx::PgPool>,
) -> RunLogger {
    match pg_pool {
        Some(pool) => {
            RunLogger::new(run_id.to_string(), region_name.to_string(), pool.clone()).await
        }
        None => RunLogger::noop(),
    }
}

/// SourcesScheduled → scrape tension sources (web + social), emit PhaseCompleted(TensionScrape).
pub fn tension_scrape_handler() -> Handler<ScoutEngineDeps> {
    on::<LifecycleEvent>()
        .id("scrape:tension")
        .filter(|e: &LifecycleEvent| {
            matches!(e, LifecycleEvent::SourcesScheduled { .. })
        })
        .then(
            |_event: Arc<LifecycleEvent>, ctx: Context<ScoutEngineDeps>| async move {
                info!("=== Phase A: Find Problems ===");
                let deps = ctx.deps();

                let phase = ScrapePhase::new(
                    deps.store.clone(),
                    deps.extractor.as_ref().expect("extractor set").clone(),
                    deps.embedder.clone(),
                    deps.fetcher.as_ref().expect("fetcher set").clone(),
                    deps.region.as_ref().expect("region set").clone(),
                    deps.run_id.clone(),
                );

                let mut state = std::mem::take(&mut *deps.state.write().await);

                // Clone sources from scheduled data before mutably borrowing state
                let (tension_web, tension_social) = {
                    let scheduled = state.scheduled.as_ref().expect("scheduled data stashed");
                    let web: Vec<SourceNode> = scheduled
                        .scheduled_sources
                        .iter()
                        .filter(|s| scheduled.tension_phase_keys.contains(&s.canonical_key))
                        .cloned()
                        .collect();
                    let social: Vec<SourceNode> = scheduled
                        .scheduled_sources
                        .iter()
                        .filter(|s| {
                            matches!(scraping_strategy(s.value()), ScrapingStrategy::Social(_))
                                && scheduled.tension_phase_keys.contains(&s.canonical_key)
                        })
                        .cloned()
                        .collect();
                    (web, social)
                };

                let region_name = deps.region.as_ref().map(|r| r.name.as_str()).unwrap_or("");
                let run_log = make_run_log(&deps.run_id, region_name, deps.pg_pool.as_ref()).await;

                let tension_web_refs: Vec<&SourceNode> = tension_web.iter().collect();
                let mut collected_events =
                    phase.run_web(&tension_web_refs, &mut state, &run_log).await;

                if !tension_social.is_empty() {
                    let tension_social_refs: Vec<&SourceNode> = tension_social.iter().collect();
                    let social_events =
                        phase.run_social(&tension_social_refs, &mut state, &run_log).await;
                    collected_events.extend(social_events);
                }

                // Put state back
                *deps.state.write().await = state;

                Ok(partition_into_events(
                    collected_events,
                    LifecycleEvent::PhaseCompleted {
                        phase: PipelinePhase::TensionScrape,
                    },
                ))
            },
        )
}

/// PhaseCompleted(MidRunDiscovery) → scrape response sources + social + topics,
/// emit PhaseCompleted(ResponseScrape).
pub fn response_scrape_handler() -> Handler<ScoutEngineDeps> {
    on::<LifecycleEvent>()
        .id("scrape:response")
        .filter(|e: &LifecycleEvent| {
            matches!(
                e,
                LifecycleEvent::PhaseCompleted { phase }
                    if matches!(phase, PipelinePhase::MidRunDiscovery)
            )
        })
        .then(
            |_event: Arc<LifecycleEvent>, ctx: Context<ScoutEngineDeps>| async move {
                info!("=== Phase B: Find Responses ===");
                let deps = ctx.deps();

                // Requires region + graph_client — skip in tests
                let (region, graph_client) = match (deps.region.as_ref(), deps.graph_client.as_ref())
                {
                    (Some(r), Some(g)) => (r, g),
                    _ => {
                        return Ok(events![LifecycleEvent::PhaseCompleted {
                            phase: PipelinePhase::ResponseScrape,
                        }]);
                    }
                };
                let writer = GraphWriter::new(graph_client.clone());

                let phase = ScrapePhase::new(
                    deps.store.clone(),
                    deps.extractor.as_ref().expect("extractor set").clone(),
                    deps.embedder.clone(),
                    deps.fetcher.as_ref().expect("fetcher set").clone(),
                    region.clone(),
                    deps.run_id.clone(),
                );

                let run_log =
                    make_run_log(&deps.run_id, &region.name, deps.pg_pool.as_ref()).await;

                let mut state = std::mem::take(&mut *deps.state.write().await);

                // Clone scheduling data before mutably borrowing state
                let (response_phase_keys, scheduled_keys, response_social_sources) = {
                    let scheduled = state.scheduled.as_ref().expect("scheduled data stashed");
                    let social: Vec<SourceNode> = scheduled
                        .scheduled_sources
                        .iter()
                        .filter(|s| {
                            matches!(scraping_strategy(s.value()), ScrapingStrategy::Social(_))
                                && scheduled.response_phase_keys.contains(&s.canonical_key)
                        })
                        .cloned()
                        .collect();
                    (
                        scheduled.response_phase_keys.clone(),
                        scheduled.scheduled_keys.clone(),
                        social,
                    )
                };

                // Reload sources from graph to pick up mid-run discoveries
                let fresh_sources = match writer
                    .get_sources_for_region(region.center_lat, region.center_lng, region.radius_km)
                    .await
                {
                    Ok(s) => s,
                    Err(e) => {
                        warn!(error = %e, "Failed to reload sources for Phase B");
                        Vec::new()
                    }
                };

                // Phase B: originally-scheduled response + never-scraped fresh discovery
                let phase_b_sources: Vec<SourceNode> = fresh_sources
                    .iter()
                    .filter(|s| {
                        response_phase_keys.contains(&s.canonical_key)
                            || (s.last_scraped.is_none()
                                && !scheduled_keys.contains(&s.canonical_key))
                    })
                    .cloned()
                    .collect();

                // Extend URL→canonical_key with fresh sources
                for s in &fresh_sources {
                    if let Some(ref url) = s.url {
                        state
                            .url_to_canonical_key
                            .entry(sanitize_url(url))
                            .or_insert_with(|| s.canonical_key.clone());
                    }
                }

                let mut collected_events = Vec::new();

                if !phase_b_sources.is_empty() {
                    info!(
                        count = phase_b_sources.len(),
                        "Phase B sources (response + fresh discovery)"
                    );
                    let phase_b_refs: Vec<&SourceNode> = phase_b_sources.iter().collect();
                    let events = phase.run_web(&phase_b_refs, &mut state, &run_log).await;
                    collected_events.extend(events);
                }

                // Response social sources
                if !response_social_sources.is_empty() {
                    let social_refs: Vec<&SourceNode> = response_social_sources.iter().collect();
                    let events =
                        phase.run_social(&social_refs, &mut state, &run_log).await;
                    collected_events.extend(events);
                }

                // Topic discovery — search social media to find new accounts
                let mut all_social_topics = state.social_topics.drain(..).collect::<Vec<_>>();
                all_social_topics.extend(state.social_expansion_topics.drain(..));
                let topic_events = phase
                    .discover_from_topics(&all_social_topics, &mut state, &run_log)
                    .await;
                collected_events.extend(topic_events);

                // Put state back
                *deps.state.write().await = state;

                Ok(partition_into_events(
                    collected_events,
                    LifecycleEvent::PhaseCompleted {
                        phase: PipelinePhase::ResponseScrape,
                    },
                ))
            },
        )
}
