//! Discovery domain activity functions: pure logic extracted from handlers.

pub(crate) mod bootstrap;
pub(crate) mod domain_filter_gate;
pub(crate) mod link_promotion;
pub(crate) mod page_triage;
pub mod source_finder;

use tracing::info;

use ai_client::Agent;
use rootsignal_common::SourceNode;
use crate::infra::embedder::TextEmbedder;
use rootsignal_graph::GraphQueries;

use source_finder::QueryEmbedding;

/// Output from source expansion — domain types only, no seesaw Events.
pub struct SourceExpansionOutput {
    pub sources: Vec<SourceNode>,
    pub social_topics: Vec<String>,
    pub query_embeddings: Vec<QueryEmbedding>,
    pub discovery_llm_calls: u32,
}

/// Run source expansion: discover new sources based on scrape findings.
///
/// Pure: no state mutation. Returns discovered sources, social topics,
/// query embeddings, and LLM call count for the caller to emit as BudgetSpent.
pub async fn discover_expansion_sources(
    graph: &dyn GraphQueries,
    region_name: Option<&str>,
    embedder: &dyn TextEmbedder,
    ai: Option<&dyn Agent>,
    budget_exhausted: bool,
) -> SourceExpansionOutput {
    let discoverer = source_finder::SourceFinder::new(
        graph,
        region_name,
        ai,
        budget_exhausted,
    )
    .with_embedder(embedder);

    let (stats, social_topics, sources, query_embeddings) = discoverer.run().await;
    if stats.actor_sources + stats.gap_sources > 0 {
        info!("{stats}");
    }

    SourceExpansionOutput {
        sources,
        social_topics,
        query_embeddings,
        discovery_llm_calls: discoverer.discovery_llm_calls(),
    }
}
