//! Scrape domain activity functions: pure logic extracted from handlers.
//!
//! Each function takes specific inputs and returns accumulated output.
//! No `&mut PipelineState` — state flows through `ScrapeOutput`.

pub(crate) mod scrape_phase;

use std::collections::HashSet;

use rootsignal_common::{scraping_strategy, ScrapingStrategy, SourceNode};
use rootsignal_graph::GraphStore;
use tracing::{info, warn};

use crate::core::aggregate::PipelineState;
use crate::infra::run_log::RunLogger;
use crate::infra::util::sanitize_url;
use self::scrape_phase::{ScrapeOutput, ScrapePhase};

pub async fn build_run_logger(
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

/// Phase A: scrape tension sources (web + social).
///
/// Pure: reads from `state`, returns `ScrapeOutput`.
pub async fn scrape_tension(
    phase: &ScrapePhase,
    state: &PipelineState,
    run_log: &RunLogger,
) -> ScrapeOutput {
    let scheduled = state.scheduled.as_ref().expect("scheduled data stashed");
    let tension_web: Vec<SourceNode> = scheduled
        .scheduled_sources
        .iter()
        .filter(|s| scheduled.tension_phase_keys.contains(&s.canonical_key))
        .cloned()
        .collect();
    let tension_social: Vec<SourceNode> = scheduled
        .scheduled_sources
        .iter()
        .filter(|s| {
            matches!(scraping_strategy(s.value()), ScrapingStrategy::Social(_))
                && scheduled.tension_phase_keys.contains(&s.canonical_key)
        })
        .cloned()
        .collect();

    let tension_web_refs: Vec<&SourceNode> = tension_web.iter().collect();
    let mut output = phase
        .scrape_web_sources(
            &tension_web_refs,
            &state.url_to_canonical_key,
            &state.actor_contexts,
            run_log,
        )
        .await;

    if !tension_social.is_empty() {
        let tension_social_refs: Vec<&SourceNode> = tension_social.iter().collect();
        let social_output = phase
            .scrape_social_sources(
                &tension_social_refs,
                &state.url_to_canonical_key,
                &state.actor_contexts,
                run_log,
            )
            .await;
        output.merge(social_output);
    }

    output
}

/// Phase B: scrape response sources + social + topic discovery.
///
/// Pure: reads from `state`, returns `ScrapeOutput`.
/// The `social_topics` parameter should be drained from state by the caller.
pub async fn scrape_response(
    phase: &ScrapePhase,
    state: &PipelineState,
    social_topics: Vec<String>,
    graph: &GraphStore,
    region: &rootsignal_common::ScoutScope,
    run_log: &RunLogger,
) -> ScrapeOutput {
    let scheduled = state.scheduled.as_ref().expect("scheduled data stashed");
    let response_phase_keys = &scheduled.response_phase_keys;
    let scheduled_keys = &scheduled.scheduled_keys;
    let response_social_sources: Vec<SourceNode> = scheduled
        .scheduled_sources
        .iter()
        .filter(|s| {
            matches!(scraping_strategy(s.value()), ScrapingStrategy::Social(_))
                && response_phase_keys.contains(&s.canonical_key)
        })
        .cloned()
        .collect();

    // Reload sources from graph to pick up mid-run discoveries
    let fresh_sources = match graph
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
                || (s.last_scraped.is_none() && !scheduled_keys.contains(&s.canonical_key))
        })
        .cloned()
        .collect();

    // Build merged url_to_canonical_key with fresh sources
    let mut url_to_ck = state.url_to_canonical_key.clone();
    let mut output = ScrapeOutput::new();

    for s in &fresh_sources {
        if let Some(ref url) = s.url {
            let clean = sanitize_url(url);
            if !state.url_to_canonical_key.contains_key(&clean) {
                output.url_mappings.insert(clean.clone(), s.canonical_key.clone());
            }
            url_to_ck
                .entry(clean)
                .or_insert_with(|| s.canonical_key.clone());
        }
    }

    // Web scrape
    if !phase_b_sources.is_empty() {
        info!(
            count = phase_b_sources.len(),
            "Phase B sources (response + fresh discovery)"
        );
        let phase_b_refs: Vec<&SourceNode> = phase_b_sources.iter().collect();
        let web_output = phase
            .scrape_web_sources(&phase_b_refs, &url_to_ck, &state.actor_contexts, run_log)
            .await;
        // Merge web URL mappings into local for subsequent calls
        url_to_ck.extend(
            web_output
                .url_mappings
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
        output.merge(web_output);
    }

    // Response social sources
    if !response_social_sources.is_empty() {
        let social_refs: Vec<&SourceNode> = response_social_sources.iter().collect();
        let social_output = phase
            .scrape_social_sources(&social_refs, &url_to_ck, &state.actor_contexts, run_log)
            .await;
        output.merge(social_output);
    }

    // Topic discovery — search social media to find new accounts
    let mut all_social_topics = social_topics;
    all_social_topics.extend(state.social_expansion_topics.iter().cloned());
    if !all_social_topics.is_empty() {
        let topic_output = phase
            .discover_from_topics(
                &all_social_topics,
                &url_to_ck,
                &state.actor_contexts,
                run_log,
            )
            .await;
        output.merge(topic_output);
    }

    output
}
