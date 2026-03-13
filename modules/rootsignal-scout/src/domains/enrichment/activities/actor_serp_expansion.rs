use rootsignal_common::types::{ActorNode, DiscoveryMethod, ScrapingStrategy, SourceNode, SourceRole};
use rootsignal_common::scraping_strategy;

pub const MAX_SERP_QUERIES_PER_RUN: usize = 5;

/// Generate SERP query sources for actors that need web presence discovery.
///
/// Eligible: exactly one source that isn't a website or RSS feed.
/// For each, emits a SourceNode whose canonical_value is a search query
/// like "Sanctuary Supply Minneapolis" — the scraper treats these as WebQuery
/// sources and runs them through Serper on the next scout run.
pub fn expand_actors_via_serp(
    actors: &[(ActorNode, Vec<SourceNode>)],
    region_name: Option<&str>,
) -> Vec<SourceNode> {
    actors
        .iter()
        .filter(|(actor, sources)| !actor.name.trim().is_empty() && needs_expansion(sources))
        .take(MAX_SERP_QUERIES_PER_RUN)
        .map(|(actor, _)| build_query_source(actor, region_name))
        .collect()
}

fn needs_expansion(sources: &[SourceNode]) -> bool {
    if sources.len() != 1 {
        return false;
    }
    !matches!(
        scraping_strategy(sources[0].value()),
        ScrapingStrategy::WebPage | ScrapingStrategy::Rss
    )
}

fn build_query_source(actor: &ActorNode, region_name: Option<&str>) -> SourceNode {
    let query = match region_name {
        Some(region) => format!("{} {}", actor.name, region),
        None => actor.name.clone(),
    };

    SourceNode::new(
        query.clone(),
        query,
        None,
        DiscoveryMethod::SignalExpansion,
        0.3,
        SourceRole::Mixed,
        None,
    )
}
