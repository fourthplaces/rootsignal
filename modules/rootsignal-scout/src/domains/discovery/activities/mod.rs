//! Discovery domain activity functions: pure logic extracted from handlers.

use tracing::info;

use crate::core::events::ScoutEvent;
use crate::discovery::source_finder;
use crate::infra::embedder::TextEmbedder;
use crate::pipeline::scrape_phase::ScrapePhase;
use rootsignal_graph::GraphWriter;

/// Output from mid-run discovery.
pub struct MidRunOutput {
    pub events: Vec<ScoutEvent>,
    pub social_topics: Vec<String>,
}

/// Run mid-run source discovery. Returns discovered source events and social topics.
///
/// Pure: no state mutation. Social topics returned for caller to stash.
pub async fn discover_mid_run(
    writer: &GraphWriter,
    region_name: &str,
    embedder: &dyn TextEmbedder,
    api_key: Option<&str>,
    budget: &crate::scheduling::budget::BudgetTracker,
) -> MidRunOutput {
    let discoverer = source_finder::SourceFinder::new(
        writer,
        region_name,
        region_name,
        api_key,
        budget,
    )
    .with_embedder(embedder);

    let (stats, social_topics, sources) = discoverer.run().await;
    if stats.actor_sources + stats.gap_sources > 0 {
        info!("{stats}");
    }

    let mut scout_events: Vec<ScoutEvent> = Vec::new();
    if !sources.is_empty() {
        scout_events.extend(ScrapePhase::register_sources_events(
            sources,
            "source_finder",
        ));
    }

    MidRunOutput {
        events: scout_events,
        social_topics,
    }
}

