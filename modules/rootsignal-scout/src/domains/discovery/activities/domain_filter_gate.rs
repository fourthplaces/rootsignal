//! Domain filter chokepoint — single gate for all source discovery.
//!
//! Every `SourcesDiscovered` event passes through this handler.
//! Sources are either auto-accepted (social, direct-action, query, admin)
//! or LLM-filtered via `filter_domains_batch`. Accepted sources emit
//! `SystemEvent::SourceRegistered`; rejected ones emit `DiscoveryEvent::SourceRejected`.

use ai_client::Agent;
use seesaw_core::Events;
use tracing::info;

use rootsignal_common::system_events::SystemEvent;
use rootsignal_common::types::{channel_type, ChannelType, SourceNode};

use crate::domains::discovery::events::DiscoveryEvent;
use crate::domains::enrichment::activities::domain_filter;
use crate::traits::SignalReader;

/// Filter a batch of proposed sources. Trusted origins and structurally-known
/// channel types bypass the LLM; web URLs go through `filter_domains_batch`.
///
/// Fail-open: if AI or region is unavailable, all sources are accepted.
pub async fn filter_discovered_sources(
    sources: Vec<SourceNode>,
    discovered_by: &str,
    region_name: Option<&str>,
    ai: Option<&dyn Agent>,
    store: &dyn SignalReader,
) -> Events {
    let mut events = Events::new();

    if sources.is_empty() {
        return events;
    }

    // Partition: auto-accept vs needs-LLM
    let mut accepted: Vec<SourceNode> = Vec::new();
    let mut needs_filter: Vec<SourceNode> = Vec::new();

    for source in sources {
        if should_auto_accept(&source, discovered_by) {
            accepted.push(source);
        } else {
            needs_filter.push(source);
        }
    }

    // LLM filter for web URL sources
    if !needs_filter.is_empty() {
        match (ai, region_name) {
            (Some(ai), Some(region)) => {
                let urls: Vec<String> = needs_filter
                    .iter()
                    .filter_map(|s| s.url.clone())
                    .collect();

                let accepted_urls = domain_filter::filter_domains_batch(
                    &urls, region, ai, store,
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
                        events.push(DiscoveryEvent::SourceRejected {
                            source,
                            reason: "Domain rejected by LLM filter".into(),
                        });
                    }
                }

                if rejected_count > 0 {
                    info!(
                        before,
                        accepted = before - rejected_count,
                        rejected = rejected_count,
                        "Domain filter applied to discovered sources"
                    );
                }
            }
            _ => {
                // Fail-open: no AI or no region → accept all
                accepted.extend(needs_filter);
            }
        }
    }

    // Emit SourceRegistered for each accepted source
    for source in accepted {
        events.push(SystemEvent::SourceRegistered {
            source_id: source.id,
            canonical_key: source.canonical_key,
            canonical_value: source.canonical_value,
            url: source.url,
            discovery_method: source.discovery_method,
            weight: source.weight,
            source_role: source.source_role,
            gap_context: source.gap_context,
        });
    }

    events
}

/// Determine whether a source should bypass LLM filtering.
fn should_auto_accept(source: &SourceNode, discovered_by: &str) -> bool {
    // Trusted origins: admin and human submissions skip filter entirely
    if matches!(discovered_by, "admin" | "human_submission") {
        return true;
    }

    // Query sources (no URL) skip filter — they're search queries, not domains
    if source.url.is_none() {
        return true;
    }

    // Social and direct-action URLs are structurally known-good
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
    use std::any::TypeId;
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

    /// Count SourceRegistered events in the output.
    fn count_registered(events: &seesaw_core::Events) -> usize {
        let system_type = TypeId::of::<SystemEvent>();
        events
            .iter()
            .filter(|e| e.type_id == system_type)
            .filter(|e| e.payload.get("type").and_then(|v| v.as_str()) == Some("source_registered"))
            .count()
    }

    #[tokio::test]
    async fn admin_origin_bypasses_filter() {
        let store = MockSignalReader::new();
        let sources = vec![web_source("https://suspicious-site.example.com")];

        let events = filter_discovered_sources(
            sources,
            "admin",
            None,
            None,
            &store,
        )
        .await;

        assert_eq!(count_registered(&events), 1, "admin sources should be auto-accepted");
    }

    #[tokio::test]
    async fn social_url_auto_accepted() {
        let store = MockSignalReader::new();
        let sources = vec![
            social_source("https://www.instagram.com/mpls_mutual_aid"),
            social_source("https://x.com/community_org"),
        ];

        let events = filter_discovered_sources(
            sources,
            "link_promoter",
            None,
            None,
            &store,
        )
        .await;

        assert_eq!(count_registered(&events), 2, "social sources should be auto-accepted");
    }

    #[tokio::test]
    async fn query_source_without_url_accepted() {
        let store = MockSignalReader::new();
        let sources = vec![query_source("Minneapolis housing crisis")];

        let events = filter_discovered_sources(
            sources,
            "engine_started",
            None,
            None,
            &store,
        )
        .await;

        assert_eq!(count_registered(&events), 1, "query sources should be auto-accepted");
    }

    #[tokio::test]
    async fn no_ai_passes_all_sources() {
        let store = MockSignalReader::new();
        let sources = vec![
            web_source("https://example.com/community"),
            web_source("https://suspicious.example.com/spam"),
        ];

        let events = filter_discovered_sources(
            sources,
            "source_finder",
            Some("Minneapolis"),
            None, // no AI
            &store,
        )
        .await;

        assert_eq!(count_registered(&events), 2, "without AI, all sources should pass (fail-open)");
    }
}
