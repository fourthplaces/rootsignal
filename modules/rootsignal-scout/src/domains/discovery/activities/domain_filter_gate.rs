//! Domain filter chokepoint — single gate for all source discovery.
//!
//! Every `SourcesDiscovered` event passes through this handler.
//! Sources are either auto-accepted (social, direct-action, query, admin)
//! or LLM-filtered via `filter_domains_batch`.

use ai_client::Agent;
use causal::Logger;

use rootsignal_common::types::{channel_type, ChannelType, SourceNode};

use crate::domains::enrichment::activities::domain_filter;
use crate::traits::SignalReader;

/// Filter a batch of proposed sources. Trusted origins and structurally-known
/// channel types bypass the LLM; web URLs go through `filter_domains_batch`.
///
/// Fail-open: if AI is unavailable, all sources are accepted.
pub async fn filter_discovered_sources(
    sources: Vec<SourceNode>,
    discovered_by: &str,
    region_name: Option<&str>,
    ai: Option<&dyn Agent>,
    store: &dyn SignalReader,
    logger: &Logger,
) -> Vec<SourceNode> {
    if sources.is_empty() {
        return Vec::new();
    }

    let mut accepted: Vec<SourceNode> = Vec::new();
    let mut needs_filter: Vec<SourceNode> = Vec::new();

    for source in sources {
        if should_auto_accept(&source, discovered_by) {
            accepted.push(source);
        } else {
            needs_filter.push(source);
        }
    }

    logger.info(format!(
        "Source filter: {} total from {discovered_by} — {} auto-accepted, {} need LLM",
        accepted.len() + needs_filter.len(),
        accepted.len(),
        needs_filter.len(),
    ));

    if !needs_filter.is_empty() {
        match ai {
            Some(ai) => {
                let urls: Vec<String> = needs_filter
                    .iter()
                    .filter_map(|s| s.url.clone())
                    .collect();

                let accepted_urls = domain_filter::filter_domains_batch(
                    &urls, region_name, ai, store, logger,
                )
                .await;

                let accepted_set: std::collections::HashSet<&str> =
                    accepted_urls.iter().map(|u| u.as_str()).collect();

                let before = needs_filter.len();
                let mut rejected_count = 0;

                for source in needs_filter {
                    let dominated_by_url = source.url.as_deref().map_or(false, |u| {
                        accepted_set.contains(u)
                    });
                    if dominated_by_url {
                        accepted.push(source);
                    } else {
                        rejected_count += 1;
                        logger.debug(format!(
                            "Source rejected by LLM filter: {}",
                            source.url.as_deref().unwrap_or(&source.canonical_key),
                        ));
                    }
                }

                if rejected_count > 0 {
                    logger.info(format!(
                        "Source filter outcome: {} accepted, {rejected_count} rejected out of {before}",
                        before - rejected_count,
                    ));
                }
            }
            None => {
                let count = needs_filter.len();
                accepted.extend(needs_filter);
                logger.info(format!("No AI available — auto-accepting all {count} sources"));
            }
        }
    }

    accepted
}

/// Determine whether a source should bypass LLM filtering.
fn should_auto_accept(source: &SourceNode, discovered_by: &str) -> bool {
    if matches!(discovered_by, "admin" | "human_submission") {
        return true;
    }

    if source.url.is_none() {
        return true;
    }

    if let Some(ref url) = source.url {
        let ct = channel_type(url);
        if matches!(ct, ChannelType::Social | ChannelType::DirectAction) {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use rootsignal_common::{DiscoveryMethod, SourceRole};
    use crate::testing::MockSignalReader;

    fn web_source(url: &str) -> SourceNode {
        SourceNode::new(
            url.to_string(),
            url.to_string(),
            Some(url.to_string()),
            DiscoveryMethod::LinkedFrom,
            0.25,
            SourceRole::Mixed,
            None,
        )
    }

    fn query_source(query: &str) -> SourceNode {
        SourceNode::new(
            query.to_string(),
            query.to_string(),
            None,
            DiscoveryMethod::ColdStart,
            0.5,
            SourceRole::Concern,
            None,
        )
    }

    fn social_source(url: &str) -> SourceNode {
        SourceNode::new(
            url.to_string(),
            url.to_string(),
            Some(url.to_string()),
            DiscoveryMethod::LinkedFrom,
            0.25,
            SourceRole::Mixed,
            None,
        )
    }

    #[tokio::test]
    async fn admin_origin_bypasses_filter() {
        let store = MockSignalReader::new();
        let sources = vec![web_source("https://suspicious-site.example.com")];

        let accepted = filter_discovered_sources(
            sources,
            "admin",
            None,
            None,
            &store,
            &Logger::new(),
        )
        .await;

        assert_eq!(accepted.len(), 1, "admin sources should be auto-accepted");
    }

    #[tokio::test]
    async fn social_url_auto_accepted() {
        let store = MockSignalReader::new();
        let sources = vec![
            social_source("https://www.instagram.com/mpls_mutual_aid"),
            social_source("https://x.com/community_org"),
        ];

        let accepted = filter_discovered_sources(
            sources,
            "link_promoter",
            None,
            None,
            &store,
            &Logger::new(),
        )
        .await;

        assert_eq!(accepted.len(), 2, "social sources should be auto-accepted");
    }

    #[tokio::test]
    async fn query_source_without_url_accepted() {
        let store = MockSignalReader::new();
        let sources = vec![query_source("Minneapolis housing crisis")];

        let accepted = filter_discovered_sources(
            sources,
            "engine_started",
            None,
            None,
            &store,
            &Logger::new(),
        )
        .await;

        assert_eq!(accepted.len(), 1, "query sources should be auto-accepted");
    }

    #[tokio::test]
    async fn no_ai_passes_all_sources() {
        let store = MockSignalReader::new();
        let sources = vec![
            web_source("https://example.com/community"),
            web_source("https://suspicious.example.com/spam"),
        ];

        let accepted = filter_discovered_sources(
            sources,
            "source_finder",
            Some("Minneapolis"),
            None,
            &store,
            &Logger::new(),
        )
        .await;

        assert_eq!(accepted.len(), 2, "without AI, all sources should pass (fail-open)");
    }
}
