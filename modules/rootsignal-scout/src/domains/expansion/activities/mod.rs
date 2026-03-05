//! Expansion domain activity functions: pure logic extracted from handlers.

pub(crate) mod expansion;

use tracing::info;

use rootsignal_common::SourceNode;
use rootsignal_graph::GraphReader;

use seesaw_core::Events;
use crate::core::aggregate::PipelineState;
use ai_client::Agent;
use crate::domains::discovery::activities::source_finder::SourceFinder;
use crate::infra::embedder::TextEmbedder;
use crate::core::engine::ScoutEngineDeps;
use self::expansion::{Expansion, ExpansionOutput};
use crate::domains::scrape::activities::{register_sources_events, ScrapeOutput};
use crate::domains::scheduling::activities::budget::BudgetTracker;

/// Output from the full expansion + end-of-run discovery activity.
pub struct ExpansionActivityOutput {
    /// Expansion output for state application.
    pub expansion: ExpansionOutput,
    /// Events from source registration.
    pub events: Events,
    /// Scrape output from end-of-run topic discovery (if any).
    pub topic_scrape: Option<ScrapeOutput>,
}

/// Run signal expansion, end-of-run discovery, and end-of-run topic scraping.
///
/// Pure: reads from `state`, returns accumulated output.
pub async fn expand_and_discover(
    expansion: &Expansion<'_>,
    deps: Option<&ScoutEngineDeps>,
    state: &PipelineState,
    graph: &GraphReader,
    region_name: &str,
    ai: Option<&dyn Agent>,
    budget: &BudgetTracker,
    embedder: &dyn TextEmbedder,
) -> ExpansionActivityOutput {
    // Signal expansion — create sources from implied queries
    let expansion_queries = state.expansion_queries.clone();
    let mut expansion_output = expansion.generate_expansion_sources(expansion_queries).await;
    let expansion_events = std::mem::replace(&mut expansion_output.events, Events::new());

    let mut collected_events = Events::new();
    collected_events.extend(expansion_events);
    if !expansion_output.sources.is_empty() {
        collected_events.extend(register_sources_events(
            expansion_output.sources.clone(),
            "signal_expansion",
        ));
    }

    // End-of-run discovery
    let end_discoverer = SourceFinder::new(
        graph,
        region_name,
        region_name,
        ai,
        budget,
    )
    .with_embedder(embedder);
    let (end_stats, end_social_topics, end_sources, finder_events) = end_discoverer.run().await;
    collected_events.extend(finder_events);
    if !end_sources.is_empty() {
        collected_events.extend(register_sources_events(
            end_sources,
            "source_finder",
        ));
    }
    if end_stats.actor_sources + end_stats.gap_sources > 0 {
        info!("{end_stats}");
    }

    // End-of-run topic discovery
    let topic_scrape = if !end_social_topics.is_empty() {
        if let Some(deps) = deps {
            info!(
                count = end_social_topics.len(),
                "Consuming end-of-run social topics"
            );
            let topic_output = crate::domains::scrape::activities::topic_discovery::discover_from_topics(
                    deps,
                    &end_social_topics,
                    &state.url_to_canonical_key,
                    &state.actor_contexts,
                )
                .await;
            Some(topic_output)
        } else {
            None
        }
    } else {
        None
    };

    ExpansionActivityOutput {
        expansion: expansion_output,
        events: collected_events,
        topic_scrape,
    }
}
