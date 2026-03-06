//! Discovery domain activity functions: pure logic extracted from handlers.

pub(crate) mod bootstrap;
pub(crate) mod domain_filter_gate;
pub(crate) mod link_promotion;
pub(crate) mod page_triage;
pub mod source_finder;

use tracing::info;

use ai_client::Agent;
use seesaw_core::Events;
use crate::domains::scheduling::activities::budget::BudgetTracker;
use crate::infra::embedder::TextEmbedder;
use crate::domains::scrape::activities::register_sources_events;
use rootsignal_graph::GraphReader;

/// Output from source expansion.
pub struct SourceExpansionOutput {
    pub events: Events,
    pub social_topics: Vec<String>,
}

/// Run source expansion: discover new sources based on tension scrape findings.
///
/// Pure: no state mutation. Social topics returned for caller to stash.
pub async fn discover_expansion_sources(
    graph: &GraphReader,
    region_name: &str,
    embedder: &dyn TextEmbedder,
    ai: Option<&dyn Agent>,
    budget: &BudgetTracker,
) -> SourceExpansionOutput {
    let discoverer = source_finder::SourceFinder::new(
        graph,
        region_name,
        region_name,
        ai,
        budget,
    )
    .with_embedder(embedder);

    let (stats, social_topics, sources, finder_events) = discoverer.run().await;
    if stats.actor_sources + stats.gap_sources > 0 {
        info!("{stats}");
    }

    let mut scout_events = Events::new();
    scout_events.extend(finder_events);
    if !sources.is_empty() {
        scout_events.extend(register_sources_events(
            sources,
            "source_finder",
        ));
    }

    SourceExpansionOutput {
        events: scout_events,
        social_topics,
    }
}

