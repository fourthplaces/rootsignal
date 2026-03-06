//! Link promotion: extract social handles and content links from scraped pages,
//! build SourceNodes for each, and attribute discovery credit.

use std::collections::{HashMap, HashSet};

use rootsignal_common::{canonical_value, DiscoveryMethod, SourceNode, SourceRole};

use crate::domains::enrichment::activities::link_promoter::{
    self, CollectedLink, PromotionConfig,
};

/// Promoted source with optional attribution to the page it was discovered on.
pub struct PromotedSource {
    pub source: SourceNode,
    pub discovered_on: Option<String>,
}

/// Group collected links by the parent page they were discovered on.
pub fn group_links_by_parent(links: &[CollectedLink]) -> HashMap<String, Vec<String>> {
    let mut by_parent: HashMap<String, Vec<String>> = HashMap::new();
    for link in links {
        by_parent
            .entry(link.discovered_on.clone())
            .or_default()
            .push(link.url.clone());
    }
    by_parent
}

/// Partition parent pages into productive (had signals) vs needs-triage (zero signals).
pub fn classify_parent_pages<'a>(
    parent_urls: impl Iterator<Item = &'a str>,
    url_to_canonical_key: &HashMap<String, String>,
    source_signal_counts: &HashMap<String, u32>,
) -> (HashSet<String>, Vec<String>) {
    let mut productive = HashSet::new();
    let mut needs_triage = Vec::new();

    for parent_url in parent_urls {
        let ck = url_to_canonical_key.get(parent_url);
        let signal_count = ck
            .and_then(|k| source_signal_counts.get(k))
            .or_else(|| source_signal_counts.get(parent_url))
            .copied()
            .unwrap_or(0);

        if signal_count > 0 {
            productive.insert(parent_url.to_string());
        } else {
            needs_triage.push(parent_url.to_string());
        }
    }

    (productive, needs_triage)
}

/// Extract social handles from all links, deduplicate, and build SourceNodes.
pub fn promote_social_handles(links: &[CollectedLink]) -> (Vec<PromotedSource>, HashSet<String>) {
    let all_urls: Vec<String> = links.iter().map(|l| l.url.clone()).collect();
    let url_to_source: HashMap<String, String> = links
        .iter()
        .map(|l| (l.url.clone(), l.discovered_on.clone()))
        .collect();
    let handles = link_promoter::extract_social_handles_from_links(&all_urls);

    let mut seen = HashSet::new();
    let mut promoted = Vec::new();
    let mut social_urls = HashSet::new();

    for (platform, handle) in &handles {
        let url = link_promoter::platform_url(platform, handle);
        social_urls.insert(url.clone());
        let cv = canonical_value(&url);
        if seen.insert(cv.clone()) {
            let discovered_on = all_urls
                .iter()
                .find(|u| {
                    let u_lower = u.to_lowercase();
                    u_lower.contains(&format!("/{handle}"))
                        || u_lower.contains(&format!("/@{handle}"))
                })
                .and_then(|u| url_to_source.get(u))
                .cloned();
            let gap = discovered_on
                .as_ref()
                .map(|src| format!("{platform:?} handle @{handle} found on {src}"))
                .unwrap_or_else(|| {
                    format!("{platform:?} handle @{handle} found on scraped page")
                });
            let source = SourceNode::new(
                cv.clone(),
                cv,
                Some(url),
                DiscoveryMethod::LinkedFrom,
                0.25,
                SourceRole::Mixed,
                Some(gap),
            );
            promoted.push(PromotedSource { source, discovered_on });
        }
    }

    (promoted, social_urls)
}

/// Extract content links from productive pages, capped per-page, skipping already-promoted URLs.
pub fn promote_content_links(
    links_by_parent: &HashMap<String, Vec<String>>,
    productive_pages: &HashSet<String>,
    already_promoted: &HashSet<String>,
    config: &PromotionConfig,
) -> Vec<PromotedSource> {
    let mut seen: HashSet<String> = already_promoted
        .iter()
        .map(|u| canonical_value(u))
        .collect();
    let mut promoted = Vec::new();

    for (parent_url, child_links) in links_by_parent {
        if !productive_pages.contains(parent_url) {
            continue;
        }
        let mut content_count = 0usize;
        for link_url in child_links {
            if already_promoted.contains(link_url) {
                continue;
            }
            if content_count >= config.max_content_links_per_source {
                break;
            }
            let cv = canonical_value(link_url);
            if seen.insert(cv.clone()) {
                let source = SourceNode::new(
                    cv.clone(),
                    cv,
                    Some(link_url.clone()),
                    DiscoveryMethod::LinkedFrom,
                    0.25,
                    SourceRole::Mixed,
                    Some(format!("Linked from {parent_url}")),
                );
                promoted.push(PromotedSource {
                    source,
                    discovered_on: Some(parent_url.clone()),
                });
                content_count += 1;
            }
        }
    }

    promoted
}

/// Build discovery credit: canonical_key → number of sources discovered from that page.
pub fn compute_discovery_credit(
    promoted: &[PromotedSource],
    url_to_canonical_key: &HashMap<String, String>,
) -> HashMap<String, u32> {
    let mut credit: HashMap<String, u32> = HashMap::new();
    for p in promoted {
        if let Some(parent_url) = &p.discovered_on {
            if let Some(ck) = url_to_canonical_key.get(parent_url) {
                *credit.entry(ck.clone()).or_default() += 1;
            }
        }
    }
    credit
}
