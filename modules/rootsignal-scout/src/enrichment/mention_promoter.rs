//! Mention promotion — create SourceNodes from @mentions and social links
//! discovered during scraping.

use std::collections::HashSet;

use anyhow::Result;
use tracing::{info, warn};

use rootsignal_common::{DiscoveryMethod, SocialPlatform, SourceNode, SourceRole};
use rootsignal_graph::GraphWriter;

pub struct PromotionConfig {
    pub max_per_source: usize,
    pub max_per_run: usize,
}

impl Default for PromotionConfig {
    fn default() -> Self {
        Self {
            max_per_source: 10,
            max_per_run: 30,
        }
    }
}

/// Promote mentioned accounts to SourceNodes in the graph.
///
/// Takes a pre-collected list of `(platform, handle, mentioned_by_source)` tuples,
/// deduplicates by `(platform, handle)`, applies the per-run cap, and upserts
/// each unique mention as a new source with MERGE semantics (idempotent).
///
/// Returns the count of newly created sources.
pub async fn promote_mentioned_accounts(
    mentioned: &[(SocialPlatform, String, String)],
    writer: &GraphWriter,
    config: &PromotionConfig,
) -> Result<u32> {
    if mentioned.is_empty() {
        return Ok(0);
    }

    // Deduplicate by (platform, handle)
    let mut seen = HashSet::new();
    let unique: Vec<&(SocialPlatform, String, String)> = mentioned
        .iter()
        .filter(|(platform, handle, _)| seen.insert((*platform, handle.clone())))
        .take(config.max_per_run)
        .collect();

    let mut created = 0u32;
    for (platform, handle, mentioned_by) in unique {
        let canonical_key = format!("{}:{}", platform_prefix(platform), handle);
        let url = platform_url(platform, handle);

        let source = SourceNode::new(
            canonical_key.clone(),
            handle.clone(),
            Some(url),
            DiscoveryMethod::SocialGraphFollow,
            0.2,
            SourceRole::Mixed,
            Some(format!("Mentioned by {mentioned_by}")),
        );

        match writer.upsert_source(&source).await {
            Ok(_) => {
                created += 1;
                info!(canonical_key, mentioned_by, "Promoted mentioned account");
            }
            Err(e) => warn!(canonical_key, error = %e, "Failed to promote mentioned account"),
        }
    }

    if created > 0 {
        info!(created, total_mentions = mentioned.len(), "Mention promotion complete");
    }

    Ok(created)
}

/// Extract social handles from a list of page links.
///
/// Matches known platform URL patterns and returns `(platform, handle)` pairs.
pub fn extract_social_handles_from_links(links: &[String]) -> Vec<(SocialPlatform, String)> {
    let mut results = Vec::new();
    for link in links {
        if let Some(pair) = parse_social_link(link) {
            results.push(pair);
        }
    }
    results
}

fn parse_social_link(url: &str) -> Option<(SocialPlatform, String)> {
    let url_lower = url.to_lowercase();

    // Instagram
    if url_lower.contains("instagram.com/") {
        return extract_handle_from_path(url, "instagram.com/")
            .map(|h| (SocialPlatform::Instagram, h));
    }

    // Twitter / X
    if url_lower.contains("twitter.com/") {
        return extract_handle_from_path(url, "twitter.com/")
            .map(|h| (SocialPlatform::Twitter, h));
    }
    if url_lower.contains("x.com/") {
        return extract_handle_from_path(url, "x.com/")
            .map(|h| (SocialPlatform::Twitter, h));
    }

    // TikTok
    if url_lower.contains("tiktok.com/@") {
        return extract_handle_from_path(url, "tiktok.com/@")
            .map(|h| (SocialPlatform::TikTok, h));
    }

    // Facebook
    if url_lower.contains("facebook.com/") {
        return extract_handle_from_path(url, "facebook.com/")
            .map(|h| (SocialPlatform::Facebook, h));
    }

    // YouTube
    // Skip YouTube — not a scrapable social platform in our pipeline

    // Bluesky
    if url_lower.contains("bsky.app/profile/") {
        return extract_handle_from_path(url, "bsky.app/profile/")
            .map(|h| (SocialPlatform::Bluesky, h));
    }

    None
}

/// Extract a handle from the path segment after the given prefix.
/// Filters out non-profile paths (e.g. /p/, /explore/, /about/).
fn extract_handle_from_path(url: &str, after: &str) -> Option<String> {
    let url_lower = url.to_lowercase();
    let idx = url_lower.find(&after.to_lowercase())? + after.len();
    let rest = &url[idx..];

    // Take the first path segment
    let handle = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .trim_start_matches('@')
        .to_string();

    if handle.is_empty() {
        return None;
    }

    // Filter out known non-profile paths
    let non_profile = [
        "p", "explore", "about", "help", "settings", "accounts",
        "stories", "reels", "reel", "tv", "hashtag", "search",
        "intent", "i", "share", "login", "signup",
    ];
    if non_profile.contains(&handle.to_lowercase().as_str()) {
        return None;
    }

    Some(handle)
}

fn platform_prefix(platform: &SocialPlatform) -> &'static str {
    match platform {
        SocialPlatform::Instagram => "instagram",
        SocialPlatform::Facebook => "facebook",
        SocialPlatform::Twitter => "twitter",
        SocialPlatform::TikTok => "tiktok",
        SocialPlatform::Reddit => "reddit",
        SocialPlatform::Bluesky => "bluesky",
    }
}

fn platform_url(platform: &SocialPlatform, handle: &str) -> String {
    match platform {
        SocialPlatform::Instagram => format!("https://instagram.com/{handle}"),
        SocialPlatform::Facebook => format!("https://facebook.com/{handle}"),
        SocialPlatform::Twitter => format!("https://x.com/{handle}"),
        SocialPlatform::TikTok => format!("https://tiktok.com/@{handle}"),
        SocialPlatform::Reddit => format!("https://reddit.com/r/{handle}"),
        SocialPlatform::Bluesky => format!("https://bsky.app/profile/{handle}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_social_handles_from_links() {
        let links = vec![
            "https://instagram.com/jane_doe".to_string(),
            "https://www.twitter.com/johndoe".to_string(),
            "https://x.com/johndoe".to_string(),
            "https://tiktok.com/@dancer123".to_string(),
            "https://facebook.com/local_org".to_string(),
            "https://bsky.app/profile/user.bsky.social".to_string(),
            "https://example.com/not-social".to_string(),
            "https://instagram.com/explore/".to_string(),
        ];
        let results = extract_social_handles_from_links(&links);
        assert_eq!(results.len(), 6);
        assert_eq!(results[0], (SocialPlatform::Instagram, "jane_doe".to_string()));
        assert_eq!(results[1], (SocialPlatform::Twitter, "johndoe".to_string()));
        assert_eq!(results[2], (SocialPlatform::Twitter, "johndoe".to_string()));
        assert_eq!(results[3], (SocialPlatform::TikTok, "dancer123".to_string()));
        assert_eq!(results[4], (SocialPlatform::Facebook, "local_org".to_string()));
        assert_eq!(results[5], (SocialPlatform::Bluesky, "user.bsky.social".to_string()));
    }

    #[test]
    fn test_extract_filters_non_profile_paths() {
        let links = vec![
            "https://instagram.com/p/abc123".to_string(),
            "https://instagram.com/explore/".to_string(),
            "https://twitter.com/intent/tweet".to_string(),
            "https://twitter.com/i/flow/login".to_string(),
            "https://instagram.com/real_account".to_string(),
        ];
        let results = extract_social_handles_from_links(&links);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], (SocialPlatform::Instagram, "real_account".to_string()));
    }

    #[test]
    fn test_dedup_canonical_key_format() {
        // Verify canonical_key format
        assert_eq!(platform_prefix(&SocialPlatform::Instagram), "instagram");
        assert_eq!(
            platform_url(&SocialPlatform::Instagram, "jane_doe"),
            "https://instagram.com/jane_doe"
        );
    }
}
