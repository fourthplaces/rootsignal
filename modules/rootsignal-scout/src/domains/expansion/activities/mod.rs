//! Expansion domain activity functions: pure logic extracted from handlers.

pub(crate) mod expansion;

use tracing::info;

use rootsignal_common::SourceNode;
use rootsignal_graph::GraphQueries;

use crate::core::aggregate::PipelineState;
use ai_client::Agent;
use crate::domains::discovery::activities::source_finder::{QueryEmbedding, SourceFinder};
use crate::infra::embedder::TextEmbedder;
use crate::core::engine::ScoutEngineDeps;
use self::expansion::{Expansion, ExpansionOutput};
use crate::domains::scrape::activities::ScrapeOutput;

/// Output from the full expansion + end-of-run discovery activity.
pub struct ExpansionActivityOutput {
    pub expansion: ExpansionOutput,
    pub discovery_sources: Vec<SourceNode>,
    pub discovery_query_embeddings: Vec<QueryEmbedding>,
    pub discovery_llm_calls: u32,
    pub topic_scrape: Option<ScrapeOutput>,
}

/// Run signal expansion, end-of-run discovery, and end-of-run topic scraping.
///
/// Pure: reads from `state`, returns accumulated output.
pub async fn expand_and_discover(
    expansion: &Expansion<'_>,
    deps: Option<&ScoutEngineDeps>,
    state: &PipelineState,
    graph: &dyn GraphQueries,
    region_name: Option<&str>,
    ai: Option<&dyn Agent>,
    budget_exhausted: bool,
    embedder: &dyn TextEmbedder,
) -> ExpansionActivityOutput {
    // Signal expansion — create sources from implied queries
    let expansion_queries = state.expansion_queries.clone();
    let expansion_output = expansion.generate_expansion_sources(expansion_queries).await;

    // End-of-run discovery
    let end_discoverer = SourceFinder::new(
        graph,
        region_name,
        ai,
        budget_exhausted,
    )
    .with_embedder(embedder);
    let (end_stats, end_social_topics, end_sources, discovery_query_embeddings) = end_discoverer.run().await;
    let discovery_llm_calls = end_discoverer.discovery_llm_calls();
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
        discovery_sources: end_sources,
        discovery_query_embeddings,
        discovery_llm_calls,
        topic_scrape,
    }
}
