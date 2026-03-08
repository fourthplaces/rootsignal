//! Link promotion: turn scraped page links into new source candidates.
//!
//! Two tracks:
//! - Social handles are promoted from ALL links (always valuable)
//! - Content links are promoted only from "productive" pages
//!   (pages that produced signals, or passed LLM triage)

use std::collections::{HashMap, HashSet};

use ai_client::Agent;
use rootsignal_common::{canonical_value, DiscoveryMethod, SourceNode, SourceRole};
use seesaw_core::Logger;
use tracing::info;

use crate::domains::enrichment::activities::link_promoter::{
    self, CollectedLink, PromotionConfig,
};

use super::page_triage::{self, PageTriageInput};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub struct PromotionResult {
    pub sources: Vec<SourceNode>,
    pub credit: HashMap<String, u32>,
}

/// Promote scraped links into source candidates.
///
/// Returns sources to register and credit attribution per parent page.
pub async fn promote_scraped_links(
    links: &[CollectedLink],
    url_to_ck: &HashMap<String, String>,
    signal_counts: &HashMap<String, u32>,
    page_previews: &HashMap<String, String>,
    ai: Option<&dyn Agent>,
    config: &PromotionConfig,
    logger: &Logger,
) -> PromotionResult {
    let by_parent = group_by_parent(links);
    logger.info(format!(
        "Link promotion: {} links from {} pages",
        links.len(),
        by_parent.len(),
    ));

    let productive = find_productive_pages(
        &by_parent, url_to_ck, signal_counts, page_previews, ai,
    ).await;

    let mut seen = HashSet::new();
    let social = build_social_sources(links, &mut seen, url_to_ck);
    let content = build_content_sources(&by_parent, &productive, &mut seen, config, url_to_ck);

    logger.info(format!(
        "Link promotion: {} social handles, {} content links from {} productive pages",
        social.len(),
        content.len(),
        productive.len(),
    ));

    let all: Vec<_> = social.into_iter().chain(content).collect();
    let credit = tally_credit(&all, url_to_ck);
    let sources = all.into_iter().map(|p| p.source).collect();

    PromotionResult { sources, credit }
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

struct Candidate {
    source: SourceNode,
    parent_url: Option<String>,
}

// ---------------------------------------------------------------------------
// Step 1: group links by the page they were found on
// ---------------------------------------------------------------------------

fn group_by_parent(links: &[CollectedLink]) -> HashMap<String, Vec<String>> {
    let mut by_parent: HashMap<String, Vec<String>> = HashMap::new();
    for link in links {
        by_parent
            .entry(link.discovered_on.clone())
            .or_default()
            .push(link.url.clone());
    }
    by_parent
}

// ---------------------------------------------------------------------------
// Step 2: classify parent pages as productive or not
// ---------------------------------------------------------------------------

/// A page is "productive" if it produced signals during scraping, or if
/// LLM triage says its outbound links point to community-relevant sources.
/// Pages with no AI available and zero signals are excluded (fail-closed).
async fn find_productive_pages(
    by_parent: &HashMap<String, Vec<String>>,
    url_to_ck: &HashMap<String, String>,
    signal_counts: &HashMap<String, u32>,
    page_previews: &HashMap<String, String>,
    ai: Option<&dyn Agent>,
) -> HashSet<String> {
    let mut productive = HashSet::new();
    let mut needs_triage = Vec::new();

    for parent_url in by_parent.keys() {
        let ck = url_to_ck.get(parent_url);
        let count = ck
            .and_then(|k| signal_counts.get(k))
            .or_else(|| signal_counts.get(parent_url))
            .copied()
            .unwrap_or(0);

        if count > 0 {
            productive.insert(parent_url.clone());
        } else {
            needs_triage.push(parent_url.clone());
        }
    }

    if let Some(ai) = ai {
        if !needs_triage.is_empty() {
            let inputs: Vec<PageTriageInput> = needs_triage
                .iter()
                .map(|url| PageTriageInput {
                    url: url.clone(),
                    content_preview: page_previews.get(url).cloned().unwrap_or_default(),
                    link_count: by_parent.get(url).map(|l| l.len()).unwrap_or(0),
                })
                .collect();

            for (url, relevant, _reason) in page_triage::triage_pages(&inputs, ai).await {
                if relevant {
                    productive.insert(url);
                }
            }
        }
    }

    productive
}

// ---------------------------------------------------------------------------
// Step 3: extract social handles → source candidates
// ---------------------------------------------------------------------------

/// Social handles are promoted from ALL links regardless of page productivity.
fn build_social_sources(
    links: &[CollectedLink],
    seen: &mut HashSet<String>,
    url_to_ck: &HashMap<String, String>,
) -> Vec<Candidate> {
    let all_urls: Vec<String> = links.iter().map(|l| l.url.clone()).collect();
    let url_to_parent: HashMap<&str, &str> = links
        .iter()
        .map(|l| (l.url.as_str(), l.discovered_on.as_str()))
        .collect();

    let handles = link_promoter::extract_social_handles_from_links(&all_urls);
    let mut candidates = Vec::new();

    for (platform, handle) in &handles {
        let Some(url) = link_promoter::platform_url(platform, handle) else {
            continue;
        };
        let cv = canonical_value(&url);
        if !seen.insert(cv.clone()) {
            continue;
        }

        let parent_url = all_urls
            .iter()
            .find(|u| {
                let lower = u.to_lowercase();
                lower.contains(&format!("/{handle}")) || lower.contains(&format!("/@{handle}"))
            })
            .and_then(|u| url_to_parent.get(u.as_str()).copied());

        let gap = match parent_url {
            Some(src) => format!("{platform:?} handle @{handle} found on {src}"),
            None => format!("{platform:?} handle @{handle} found on scraped page"),
        };

        let discovered_from_key = parent_url.and_then(|u| url_to_ck.get(u)).cloned();
        let mut source = SourceNode::new(
            cv.clone(), cv, Some(url),
            DiscoveryMethod::LinkedFrom, 0.25, SourceRole::Mixed, Some(gap),
        );
        source.discovered_from_key = discovered_from_key;
        candidates.push(Candidate {
            source,
            parent_url: parent_url.map(str::to_string),
        });
    }

    candidates
}

// ---------------------------------------------------------------------------
// Step 4: extract content links from productive pages → source candidates
// ---------------------------------------------------------------------------

fn build_content_sources(
    by_parent: &HashMap<String, Vec<String>>,
    productive: &HashSet<String>,
    seen: &mut HashSet<String>,
    config: &PromotionConfig,
    url_to_ck: &HashMap<String, String>,
) -> Vec<Candidate> {
    let mut candidates = Vec::new();

    for (parent_url, child_links) in by_parent {
        if !productive.contains(parent_url) {
            continue;
        }
        let discovered_from_key = url_to_ck.get(parent_url).cloned();
        let mut count = 0usize;
        for link_url in child_links {
            if count >= config.max_content_links_per_source {
                break;
            }
            let cv = canonical_value(link_url);
            if !seen.insert(cv.clone()) {
                continue;
            }
            let mut source = SourceNode::new(
                cv.clone(), cv, Some(link_url.clone()),
                DiscoveryMethod::LinkedFrom, 0.25, SourceRole::Mixed,
                Some(format!("Linked from {parent_url}")),
            );
            source.discovered_from_key = discovered_from_key.clone();
            candidates.push(Candidate {
                source,
                parent_url: Some(parent_url.clone()),
            });
            count += 1;
        }
    }

    candidates
}

// ---------------------------------------------------------------------------
// Step 5: attribute credit to parent pages that produced new sources
// ---------------------------------------------------------------------------

fn tally_credit(
    candidates: &[Candidate],
    url_to_ck: &HashMap<String, String>,
) -> HashMap<String, u32> {
    let mut credit: HashMap<String, u32> = HashMap::new();
    for c in candidates {
        if let Some(parent) = &c.parent_url {
            if let Some(ck) = url_to_ck.get(parent) {
                *credit.entry(ck.clone()).or_default() += 1;
            }
        }
    }
    credit
}
