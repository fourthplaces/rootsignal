//! Link promotion — create SourceNodes from outbound links discovered during scraping.
//!
//! Replaces `mention_promoter`. Instead of extracting only social handles, extracts
//! ALL outbound URLs from page links, filters out non-content junk (by scheme and
//! file extension only), and promotes each as a SourceNode. Dormancy self-heals
//! noisy sources after 3 empty runs.

use std::collections::HashSet;

use tracing::info;

use rootsignal_common::{canonical_value, DiscoveryMethod, SocialPlatform, SourceNode, SourceRole};
use crate::infra::util::sanitize_url;

/// A link discovered during scraping, used by `promote_links` to create new sources.
#[derive(Clone)]
pub struct CollectedLink {
    pub url: String,
    pub discovered_on: String,
}

pub struct PromotionConfig {
    pub max_per_source: usize,
    pub max_per_run: usize,
}

impl Default for PromotionConfig {
    fn default() -> Self {
        Self {
            max_per_source: 20,
            max_per_run: 100,
        }
    }
}

const SKIP_PREFIXES: &[&str] = &["mailto:", "tel:", "javascript:", "#", "data:"];

const SKIP_EXTENSIONS: &[&str] = &[
    ".css", ".js", ".png", ".jpg", ".jpeg", ".gif", ".svg", ".woff", ".woff2", ".ico", ".webp",
    ".mp3", ".mp4",
];


/// Extract all content-worthy URLs from a list of page links.
///
/// Filters out non-content URLs by scheme prefix and file extension,
/// strips tracking parameters, and deduplicates by `canonical_value()`.
pub fn extract_links(page_links: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut results = Vec::new();

    for link in page_links {
        let trimmed = link.trim();

        // Skip non-content schemes
        if SKIP_PREFIXES.iter().any(|p| trimmed.starts_with(p)) {
            continue;
        }

        // Must be http(s)
        if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
            continue;
        }

        // Skip non-content file extensions
        let path_lower = trimmed.split('?').next().unwrap_or(trimmed).to_lowercase();
        if SKIP_EXTENSIONS.iter().any(|ext| path_lower.ends_with(ext)) {
            continue;
        }

        let cleaned = sanitize_url(trimmed);
        let cv = canonical_value(&cleaned);
        if seen.insert(cv) {
            results.push(cleaned);
        }
    }

    results
}

/// Build SourceNodes from discovered links.
///
/// Each `CollectedLink` carries the discovering source's coordinates. The promoted
/// source inherits those coordinates — not the region center.
///
/// Returns the SourceNodes to be registered through the engine.
pub fn promote_links(links: &[CollectedLink], config: &PromotionConfig) -> Vec<SourceNode> {
    if links.is_empty() {
        return Vec::new();
    }

    // Deduplicate by canonical_value, keeping the first occurrence's coords
    let mut seen = HashSet::new();
    let unique: Vec<&CollectedLink> = links
        .iter()
        .filter(|link| seen.insert(canonical_value(&link.url)))
        .take(config.max_per_run)
        .collect();

    let mut sources = Vec::new();
    for link in unique {
        let cv = canonical_value(&link.url);

        let source = SourceNode::new(
            cv.clone(),
            canonical_value(&link.url),
            Some(link.url.clone()),
            DiscoveryMethod::LinkedFrom,
            0.25,
            SourceRole::Mixed,
            Some(format!("Linked from {}", link.discovered_on)),
        );
        sources.push(source);
        info!(
            canonical_key = cv,
            discovered_on = link.discovered_on,
            "Promoted linked URL"
        );
    }

    if !sources.is_empty() {
        info!(
            created = sources.len(),
            total_links = links.len(),
            "Link promotion complete"
        );
    }

    sources
}

// ---------------------------------------------------------------------------
// Social handle helpers (migrated from mention_promoter)
// ---------------------------------------------------------------------------

/// Build a canonical URL for a social platform handle.
pub fn platform_url(platform: &SocialPlatform, handle: &str) -> String {
    match platform {
        SocialPlatform::Instagram => format!("https://instagram.com/{handle}"),
        SocialPlatform::Facebook => format!("https://facebook.com/{handle}"),
        SocialPlatform::Twitter => format!("https://x.com/{handle}"),
        SocialPlatform::TikTok => format!("https://tiktok.com/@{handle}"),
        SocialPlatform::Reddit => format!("https://reddit.com/r/{handle}"),
        SocialPlatform::Bluesky => format!("https://bsky.app/profile/{handle}"),
    }
}

fn parse_social_link(url: &str) -> Option<(SocialPlatform, String)> {
    let url_lower = url.to_lowercase();

    if url_lower.contains("instagram.com/") {
        return extract_handle_from_path(url, "instagram.com/")
            .map(|h| (SocialPlatform::Instagram, h));
    }
    if url_lower.contains("twitter.com/") {
        return extract_handle_from_path(url, "twitter.com/").map(|h| (SocialPlatform::Twitter, h));
    }
    if url_lower.contains("x.com/") {
        return extract_handle_from_path(url, "x.com/").map(|h| (SocialPlatform::Twitter, h));
    }
    if url_lower.contains("tiktok.com/@") {
        return extract_handle_from_path(url, "tiktok.com/@").map(|h| (SocialPlatform::TikTok, h));
    }
    if url_lower.contains("facebook.com/") {
        return extract_handle_from_path(url, "facebook.com/")
            .map(|h| (SocialPlatform::Facebook, h));
    }
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

    let handle = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .trim_start_matches('@')
        .to_string();

    if handle.is_empty() {
        return None;
    }

    let non_profile = [
        "p", "explore", "about", "help", "settings", "accounts", "stories", "reels", "reel", "tv",
        "hashtag", "search", "intent", "i", "share", "login", "signup",
    ];
    if non_profile.contains(&handle.to_lowercase().as_str()) {
        return None;
    }

    Some(handle)
}

/// Extract social handles from a list of page links (used by `run_social`).
pub fn extract_social_handles_from_links(links: &[String]) -> Vec<(SocialPlatform, String)> {
    let mut results = Vec::new();
    for link in links {
        if let Some(pair) = parse_social_link(link) {
            results.push(pair);
        }
    }
    results
}

#[allow(dead_code)]
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
        assert_eq!(
            results[0],
            (SocialPlatform::Instagram, "jane_doe".to_string())
        );
        assert_eq!(results[1], (SocialPlatform::Twitter, "johndoe".to_string()));
        assert_eq!(results[2], (SocialPlatform::Twitter, "johndoe".to_string()));
        assert_eq!(
            results[3],
            (SocialPlatform::TikTok, "dancer123".to_string())
        );
        assert_eq!(
            results[4],
            (SocialPlatform::Facebook, "local_org".to_string())
        );
        assert_eq!(
            results[5],
            (SocialPlatform::Bluesky, "user.bsky.social".to_string())
        );
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
        assert_eq!(
            results[0],
            (SocialPlatform::Instagram, "real_account".to_string())
        );
    }

    #[test]
    fn test_platform_url_helper() {
        assert_eq!(
            platform_url(&SocialPlatform::Instagram, "jane_doe"),
            "https://instagram.com/jane_doe"
        );
        assert_eq!(
            platform_url(&SocialPlatform::Twitter, "johndoe"),
            "https://x.com/johndoe"
        );
        assert_eq!(
            platform_url(&SocialPlatform::TikTok, "dancer"),
            "https://tiktok.com/@dancer"
        );
        assert_eq!(
            platform_url(&SocialPlatform::Facebook, "local_org"),
            "https://facebook.com/local_org"
        );
        assert_eq!(
            platform_url(&SocialPlatform::Reddit, "mutualaid"),
            "https://reddit.com/r/mutualaid"
        );
        assert_eq!(
            platform_url(&SocialPlatform::Bluesky, "user.bsky.social"),
            "https://bsky.app/profile/user.bsky.social"
        );
    }

    #[test]
    fn test_non_content_filtering() {
        let links = vec![
            "mailto:test@example.com".to_string(),
            "javascript:void(0)".to_string(),
            "tel:+15551234567".to_string(),
            "#anchor".to_string(),
            "data:text/html,<h1>hi</h1>".to_string(),
            "https://example.com/style.css".to_string(),
            "https://example.com/app.js".to_string(),
            "https://example.com/logo.png".to_string(),
            "https://example.com/photo.jpg".to_string(),
            "https://example.com/font.woff2".to_string(),
            "https://example.com/real-page".to_string(),
        ];
        let results = extract_links(&links);
        assert_eq!(results.len(), 1);
        assert!(results[0].contains("real-page"));
    }

    #[test]
    fn test_tracking_param_stripping_via_sanitize_url() {
        // extract_links delegates to sanitize_url for tracking param removal
        let links = vec![
            "https://example.com/page?utm_source=ig&utm_medium=social&fbclid=abc123&important=yes"
                .to_string(),
        ];
        let results = extract_links(&links);
        assert_eq!(results.len(), 1);
        assert!(results[0].contains("important=yes"));
        assert!(!results[0].contains("utm_source"));
        assert!(!results[0].contains("fbclid"));
    }

    #[test]
    fn test_tracking_params_all_removed() {
        let links = vec!["https://example.com/page?utm_source=ig&fbclid=abc".to_string()];
        let results = extract_links(&links);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], "https://example.com/page");
    }

    #[test]
    fn test_dedup_same_url_different_tracking() {
        let links = vec![
            "https://example.com/page?utm_source=ig".to_string(),
            "https://example.com/page?utm_source=twitter".to_string(),
            "https://example.com/page".to_string(),
        ];
        let results = extract_links(&links);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_mixed_linktree_page() {
        let links = vec![
            "https://instagram.com/mutual_aid_mpls".to_string(),
            "https://x.com/mpls_aid".to_string(),
            "https://docs.google.com/document/d/1abc/edit".to_string(),
            "https://gofundme.com/f/help-my-family".to_string(),
            "https://www.eventbrite.com/e/community-dinner-123".to_string(),
            "https://anotherorg.org/resources".to_string(),
            "https://example.com/flyer.pdf".to_string(),
            "mailto:contact@org.com".to_string(),
        ];
        let results = extract_links(&links);
        // All http(s) links except mailto should be extracted (including .pdf — not in SKIP_EXTENSIONS)
        assert_eq!(results.len(), 7);
    }

    #[test]
    fn test_non_http_schemes_skipped() {
        let links = vec![
            "data:text/html,test".to_string(),
            "tel:5551234".to_string(),
            "#section-2".to_string(),
            "ftp://files.example.com/doc".to_string(),
        ];
        let results = extract_links(&links);
        assert!(results.is_empty());
    }

    // -----------------------------------------------------------------------
    // URL normalization: canonical_value vs sanitize_url
    //
    // canonical_value (rootsignal-common) — identity key for sources/dedup.
    //   Preserves ALL query params. Intentionally different from sanitize_url.
    //
    // sanitize_url (scout::infra::util) — the single URL cleaner for scout.
    //   Strips tracking params (utm_*, fbclid, gclid, si, source, etc.).
    // -----------------------------------------------------------------------

    #[test]
    fn canonical_value_preserves_tracking_params_sanitize_url_strips_them() {
        use crate::infra::util::sanitize_url;

        let url = "https://example.com/page?utm_source=ig&si=abc&important=yes";

        let cv = canonical_value(url);
        let sanitized = sanitize_url(url);

        // canonical_value: identity key — preserves everything
        assert!(
            cv.contains("utm_source"),
            "canonical_value preserves tracking params"
        );
        assert!(cv.contains("si="), "canonical_value preserves si param");

        // sanitize_url: strips tracking params, keeps the rest
        assert!(
            !sanitized.contains("utm_source"),
            "sanitize_url strips utm params"
        );
        assert!(!sanitized.contains("si="), "sanitize_url strips si param");
        assert!(
            sanitized.contains("important=yes"),
            "sanitize_url keeps non-tracking params"
        );
    }
}

