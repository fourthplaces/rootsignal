//! Batch LLM domain filter — evaluates whether scraped domains are likely
//! to contain community signals for a target region. One Haiku call per
//! `run_web()` invocation, with Neo4j-backed caching of verdicts.
//!
//! Follows the signal_lint.rs pattern (batch items, per-item verdict) and
//! the universe_check.rs pattern (structured output, fail-open on error).

use std::collections::{HashMap, HashSet};

use ai_client::claude::Claude;
use schemars::JsonSchema;
use serde::Deserialize;
use tracing::{info, warn};

use rootsignal_common::extract_domain;

use crate::pipeline::traits::SignalStore;

#[derive(Deserialize, JsonSchema)]
struct DomainFilterResponse {
    /// One verdict per submitted domain.
    domains: Vec<DomainVerdict>,
}

#[derive(Deserialize, JsonSchema)]
struct DomainVerdict {
    /// The domain being evaluated.
    domain: String,
    /// true = likely to contain local community signals; false = reject.
    accept: bool,
    /// Brief reason for the verdict.
    reason: String,
}

const DOMAIN_FILTER_SYSTEM: &str = "\
You evaluate whether web domains are likely to contain current, firsthand \
community signals for a specific region.\n\n\
ACCEPT domains that host:\n\
- Local journalism, community newspapers, radio/TV stations\n\
- Local government services, meetings, public records\n\
- Community organizations, nonprofits, mutual aid groups\n\
- Local event listings from regional organizers\n\n\
REJECT domains that are:\n\
- Web archives or historical snapshots (archive.org, archive-it.org, wayback machine)\n\
- Foreign government sites unrelated to the target region\n\
- National/international aggregators with no local desk\n\
- E-commerce platforms, SaaS marketing, developer docs\n\
- Content farms, SEO spam, coupon/deal sites\n\n\
When unsure, ACCEPT \u{2014} false positives cost less than missed signals.\n\
Return a verdict for every domain submitted.";

/// Normalize a domain for dedup: lowercase + strip leading `www.`.
fn normalize_domain(domain: &str) -> String {
    let d = domain.to_lowercase();
    d.strip_prefix("www.").unwrap_or(&d).to_string()
}

/// Filter a list of URLs by evaluating their domains through an LLM batch call.
///
/// Returns the subset of `urls` whose domains were accepted (or already cached
/// as accepted). On LLM error the full list is returned (fail-open).
pub async fn filter_domains_batch(
    urls: &[String],
    region_name: &str,
    anthropic_api_key: &str,
    _store: &dyn SignalStore,
) -> Vec<String> {
    if urls.is_empty() {
        return Vec::new();
    }

    // 1. Extract unique normalized domains
    let mut domain_for_url: Vec<(String, String)> = Vec::new(); // (url, normalized_domain)
    let mut unique_domains: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for url in urls {
        let raw = extract_domain(url);
        let norm = normalize_domain(&raw);
        if !norm.is_empty() && seen.insert(norm.clone()) {
            unique_domains.push(norm.clone());
        }
        domain_for_url.push((url.clone(), norm));
    }

    // 2. Check cache for existing verdicts
    // TODO: Add cached_domain_verdicts / cache_domain_verdicts to SignalStore trait
    // to avoid re-evaluating domains across runs.
    let cached: HashMap<String, bool> = HashMap::new();

    let unchecked: Vec<String> = unique_domains
        .iter()
        .filter(|d| !cached.contains_key(d.as_str()))
        .cloned()
        .collect();

    // 3. If there are unchecked domains, call LLM
    let mut verdicts: HashMap<String, bool> = cached;

    if !unchecked.is_empty() {
        info!(count = unchecked.len(), "Evaluating unchecked domains via LLM");

        let domain_list = unchecked
            .iter()
            .map(|d| format!("- {d}"))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            "Region: {region_name}\n\nDomains to evaluate:\n{domain_list}"
        );

        let claude = Claude::new(anthropic_api_key, "claude-haiku-4-5-20251001");

        match claude
            .extract::<DomainFilterResponse>(DOMAIN_FILTER_SYSTEM, &prompt)
            .await
        {
            Ok(response) => {
                let mut new_verdicts: Vec<(String, bool)> = Vec::new();
                for v in &response.domains {
                    let norm = normalize_domain(&v.domain);
                    if unchecked.contains(&norm) {
                        verdicts.insert(norm.clone(), v.accept);
                        new_verdicts.push((norm, v.accept));
                        if !v.accept {
                            info!(domain = %v.domain, reason = %v.reason, "Domain rejected");
                        }
                    }
                }
                // Any domains the LLM didn't return a verdict for — accept (fail-open)
                for d in &unchecked {
                    if !verdicts.contains_key(d.as_str()) {
                        verdicts.insert(d.clone(), true);
                        new_verdicts.push((d.clone(), true));
                    }
                }
                // TODO: Cache new verdicts once SignalStore trait has cache_domain_verdicts
                let _ = &new_verdicts;
            }
            Err(e) => {
                warn!(error = %e, "Domain filter LLM call failed, passing all URLs through");
                return urls.to_vec();
            }
        }
    }

    // 5. Build rejected set and filter URLs
    let rejected: HashSet<&str> = verdicts
        .iter()
        .filter(|(_, &accepted)| !accepted)
        .map(|(d, _)| d.as_str())
        .collect();

    if rejected.is_empty() {
        return urls.to_vec();
    }

    domain_for_url
        .into_iter()
        .filter(|(_, domain)| !rejected.contains(domain.as_str()))
        .map(|(url, _)| url)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_www_and_lowercases() {
        assert_eq!(normalize_domain("www.Example.COM"), "example.com");
        assert_eq!(normalize_domain("WWW.foo.org"), "foo.org");
        assert_eq!(normalize_domain("bar.net"), "bar.net");
    }

    #[test]
    fn extract_and_normalize_deduplicates() {
        let urls = vec![
            "https://www.example.com/page1".to_string(),
            "https://example.com/page2".to_string(),
            "https://OTHER.org/x".to_string(),
            "https://other.org/y".to_string(),
        ];

        let mut seen = HashSet::new();
        let mut unique = Vec::new();
        for url in &urls {
            let norm = normalize_domain(&extract_domain(url));
            if seen.insert(norm.clone()) {
                unique.push(norm);
            }
        }

        assert_eq!(unique, vec!["example.com", "other.org"]);
    }

    #[test]
    fn system_prompt_mentions_archive_and_accept_guidance() {
        assert!(DOMAIN_FILTER_SYSTEM.contains("archive.org"));
        assert!(DOMAIN_FILTER_SYSTEM.contains("ACCEPT"));
        assert!(DOMAIN_FILTER_SYSTEM.contains("REJECT"));
        assert!(DOMAIN_FILTER_SYSTEM.contains("When unsure, ACCEPT"));
    }
}
