//! Lightweight LLM triage for zero-signal pages: do their outbound links
//! lead to community-relevant sources?
//!
//! Called by `link_promotion` for pages that produced no signals during scraping.
//! Pages that DID produce signals are auto-qualified — no triage needed.

use ai_client::{ai_extract, Agent};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

/// Input for a single page to triage.
pub struct PageTriageInput {
    pub url: String,
    pub content_preview: String,
    pub link_count: usize,
}

#[derive(Debug, Serialize)]
struct TriageItem {
    url: String,
    content_preview: String,
    link_count: usize,
}

#[derive(Deserialize, JsonSchema)]
struct TriageResponse {
    pages: Vec<TriageVerdict>,
}

#[derive(Deserialize, JsonSchema)]
struct TriageVerdict {
    url: String,
    relevant: bool,
    reason: String,
}

const TRIAGE_SYSTEM: &str = "\
You evaluate whether web pages link to community-relevant sources.

RELEVANT: resource directories, mutual aid networks, org partner/links pages, \
government service directories, community hub pages linking to organizations.

IRRELEVANT: generic corporate homepages, news articles (links are to other articles), \
e-commerce/product pages, pages where outbound links are mostly internal/same-domain.

For each page, assess whether its outbound links likely lead to community sources. \
Return a verdict for every page submitted.";

/// Batch-triage zero-signal pages via LLM.
///
/// Returns `(url, relevant, reason)` for each input page.
/// Fail-closed: on LLM error, all pages are marked irrelevant.
pub async fn triage_pages(
    pages: &[PageTriageInput],
    ai: &dyn Agent,
) -> Vec<(String, bool, String)> {
    if pages.is_empty() {
        return Vec::new();
    }

    let items: Vec<TriageItem> = pages
        .iter()
        .map(|p| TriageItem {
            url: p.url.clone(),
            content_preview: p.content_preview.clone(),
            link_count: p.link_count,
        })
        .collect();

    let prompt = serde_json::to_string(&items).unwrap_or_default();

    match ai_extract::<TriageResponse>(ai, TRIAGE_SYSTEM, &prompt).await {
        Ok(response) => {
            let mut results = Vec::new();
            // Index response by URL for lookup
            let mut verdict_map: std::collections::HashMap<String, (bool, String)> =
                response.pages.into_iter()
                    .map(|v| (v.url, (v.relevant, v.reason)))
                    .collect();

            for page in pages {
                if let Some((relevant, reason)) = verdict_map.remove(&page.url) {
                    results.push((page.url.clone(), relevant, reason));
                } else {
                    // LLM didn't return a verdict for this page — fail closed
                    results.push((
                        page.url.clone(),
                        false,
                        "no verdict returned by triage".into(),
                    ));
                }
            }

            let relevant_count = results.iter().filter(|(_, r, _)| *r).count();
            info!(
                total = results.len(),
                relevant = relevant_count,
                "Page triage complete"
            );
            results
        }
        Err(e) => {
            warn!(error = %e, pages = pages.len(), "Page triage LLM call failed, rejecting all");
            pages
                .iter()
                .map(|p| (p.url.clone(), false, format!("triage error: {e}")))
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triage_system_prompt_mentions_relevant_and_irrelevant() {
        assert!(TRIAGE_SYSTEM.contains("RELEVANT"));
        assert!(TRIAGE_SYSTEM.contains("IRRELEVANT"));
        assert!(TRIAGE_SYSTEM.contains("community"));
    }
}
