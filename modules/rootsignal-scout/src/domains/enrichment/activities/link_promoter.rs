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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CollectedLink {
    pub url: String,
    pub discovered_on: String,
}

pub struct PromotionConfig {
    pub max_per_source: usize,
    pub max_per_run: usize,
    /// Maximum number of non-social content links to promote per parent page.
    pub max_content_links_per_source: usize,
}

impl Default for PromotionConfig {
    fn default() -> Self {
        Self {
            max_per_source: 20,
            max_per_run: 100,
            max_content_links_per_source: 10,
        }
    }
}

// ---------------------------------------------------------------------------
// Pass 2 — Static asset gate
// ---------------------------------------------------------------------------

const STATIC_EXTENSIONS: &[&str] = &[
    ".css", ".js", ".json", ".xml", ".webmanifest", ".map",
    ".png", ".jpg", ".jpeg", ".gif", ".svg", ".webp", ".ico", ".avif",
    ".woff", ".woff2", ".ttf", ".eot",
    ".mp3", ".mp4", ".webm", ".ogg",
    ".pdf", ".zip", ".gz", ".tar",
    ".txt", ".csv", ".rss", ".atom",
];

const ASSET_PATH_PATTERNS: &[&str] = &[
    "/_next/", "/static/", "/assets/", "/dist/", "/build/",
    "/wp-includes/", "/wp-content/plugins/", "/wp-content/themes/",
    "/cdn-cgi/", "/wp-json/", "/xmlrpc", "/cgi-bin/",
];

// ---------------------------------------------------------------------------
// Pass 3 — Infrastructure host gate
// ---------------------------------------------------------------------------

const INFRA_SUBDOMAIN_PREFIXES: &[&str] = &[
    "assets.", "static.", "cdn.", "api.", "fonts.", "analytics.",
    "accounts.", "login.", "auth.",
];

const INFRA_DOMAINS: &[&str] = &[
    "googleapis.com", "gstatic.com", "googletagmanager.com",
    "google-analytics.com", "doubleclick.net", "cloudflare.com",
    "cdn.jsdelivr.net", "unpkg.com", "bootstrapcdn.com", "fontawesome.com",
    "w3.org", "ietf.org", "iana.org", "schema.org", "ogp.me", "xmlns.com",
    "purl.org", "dublincore.org", "rdfs.org",
    "segment.com", "hotjar.com", "newrelic.com", "sentry.io",
];


/// Extract the host from a lowercased URL, stripping `www.` prefix and port.
fn extract_host(url_lower: &str) -> Option<&str> {
    let after_scheme = url_lower.strip_prefix("https://")
        .or_else(|| url_lower.strip_prefix("http://"))?;
    let host = after_scheme.split('/').next()?;
    let host = host.split(':').next()?;
    Some(host.strip_prefix("www.").unwrap_or(host))
}

fn is_infra_domain(host: &str) -> bool {
    INFRA_DOMAINS.iter().any(|d| host == *d || host.ends_with(&format!(".{d}")))
}

fn is_infra_subdomain(host: &str) -> bool {
    INFRA_SUBDOMAIN_PREFIXES.iter().any(|prefix| host.starts_with(prefix))
}

/// Percent-decode a path for pattern matching.
/// Only decodes `%2F` and similar path-relevant sequences.
fn decode_path(path: &str) -> String {
    percent_encoding::percent_decode_str(path)
        .decode_utf8_lossy()
        .to_lowercase()
}

/// Three-pass structural filter for URLs discovered during scraping.
///
/// Pass 1 — Scheme gate: only http/https.
/// Pass 2 — Static asset gate: file extensions and build-tooling paths.
/// Pass 3 — Infrastructure host gate: infra subdomains and universal infra domains.
///
/// Then: sanitize tracking params, dedup by canonical_value.
pub fn extract_links(page_links: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut results = Vec::new();

    for link in page_links {
        let trimmed = link.trim();

        // Pass 1 — Scheme gate
        if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
            continue;
        }

        let url_lower = trimmed.to_lowercase();

        // Pass 2 — Static asset gate
        let path = url_lower.split('?').next().unwrap_or(&url_lower);
        let decoded_path = decode_path(path);

        if STATIC_EXTENSIONS.iter().any(|ext| decoded_path.ends_with(ext)) {
            continue;
        }
        if ASSET_PATH_PATTERNS.iter().any(|pat| decoded_path.contains(pat)) {
            continue;
        }

        // Pass 3 — Infrastructure host gate
        if let Some(host) = extract_host(&url_lower) {
            if is_infra_subdomain(host) || is_infra_domain(host) {
                continue;
            }
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
/// Returns None if the handle doesn't look like a valid social handle.
pub fn platform_url(platform: &SocialPlatform, handle: &str) -> Option<String> {
    let clean = clean_handle(handle)?;
    Some(match platform {
        SocialPlatform::Instagram => format!("https://instagram.com/{clean}"),
        SocialPlatform::Facebook => format!("https://facebook.com/{clean}"),
        SocialPlatform::Twitter => format!("https://x.com/{clean}"),
        SocialPlatform::TikTok => format!("https://tiktok.com/@{clean}"),
        SocialPlatform::Reddit => format!("https://reddit.com/r/{clean}"),
        SocialPlatform::Bluesky => format!("https://bsky.app/profile/{clean}"),
    })
}

/// Extract a valid handle from raw text. Handles are alphanumeric with
/// dots, underscores, and hyphens (e.g. @bob.smith, jane_doe, my-org).
/// Returns None if nothing valid remains after cleaning.
fn clean_handle(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_start_matches('@');
    let clean: String = trimmed
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '.' || *c == '_' || *c == '-')
        .collect();
    let clean = clean.trim_end_matches(['.', ',', '-']);
    if clean.is_empty() {
        None
    } else {
        Some(clean.to_string())
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

/// Extract social handles from a list of page links.
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
    fn valid_handles_produce_urls() {
        assert_eq!(
            platform_url(&SocialPlatform::Instagram, "jane_doe"),
            Some("https://instagram.com/jane_doe".into())
        );
        assert_eq!(
            platform_url(&SocialPlatform::Twitter, "johndoe"),
            Some("https://x.com/johndoe".into())
        );
        assert_eq!(
            platform_url(&SocialPlatform::TikTok, "dancer"),
            Some("https://tiktok.com/@dancer".into())
        );
        assert_eq!(
            platform_url(&SocialPlatform::Facebook, "local_org"),
            Some("https://facebook.com/local_org".into())
        );
        assert_eq!(
            platform_url(&SocialPlatform::Reddit, "mutualaid"),
            Some("https://reddit.com/r/mutualaid".into())
        );
        assert_eq!(
            platform_url(&SocialPlatform::Bluesky, "user.bsky.social"),
            Some("https://bsky.app/profile/user.bsky.social".into())
        );
    }

    #[test]
    fn trailing_punctuation_stripped_from_handle() {
        assert_eq!(
            platform_url(&SocialPlatform::Instagram, "maskblocmsp."),
            Some("https://instagram.com/maskblocmsp".into())
        );
        assert_eq!(
            platform_url(&SocialPlatform::Instagram, "sanctuarysupplydepot,"),
            Some("https://instagram.com/sanctuarysupplydepot".into())
        );
        assert_eq!(
            platform_url(&SocialPlatform::Twitter, "user.."),
            Some("https://x.com/user".into())
        );
    }

    #[test]
    fn whitespace_and_at_sign_cleaned() {
        assert_eq!(
            platform_url(&SocialPlatform::Instagram, " @handle "),
            Some("https://instagram.com/handle".into())
        );
    }

    #[test]
    fn mid_handle_period_preserved() {
        assert_eq!(
            platform_url(&SocialPlatform::Instagram, "bob.smith"),
            Some("https://instagram.com/bob.smith".into())
        );
    }

    #[test]
    fn empty_handle_returns_none() {
        assert_eq!(platform_url(&SocialPlatform::Instagram, ""), None);
        assert_eq!(platform_url(&SocialPlatform::Instagram, "..."), None);
        assert_eq!(platform_url(&SocialPlatform::Instagram, ",,,"), None);
    }

    // -----------------------------------------------------------------------
    // Pass 1 — Scheme gate
    // -----------------------------------------------------------------------

    #[test]
    fn non_http_schemes_rejected() {
        let links = vec![
            "mailto:test@example.com".to_string(),
            "javascript:void(0)".to_string(),
            "tel:+15551234567".to_string(),
            "#anchor".to_string(),
            "data:text/html,<h1>hi</h1>".to_string(),
            "ftp://files.example.com/doc".to_string(),
        ];
        assert!(extract_links(&links).is_empty());
    }

    #[test]
    fn http_and_https_accepted() {
        let links = vec![
            "http://example.com/page".to_string(),
            "https://example.com/other".to_string(),
        ];
        assert_eq!(extract_links(&links).len(), 2);
    }

    // -----------------------------------------------------------------------
    // Pass 2 — Static asset gate
    // -----------------------------------------------------------------------

    #[test]
    fn static_file_extensions_rejected() {
        let links = vec![
            "https://example.com/style.css".to_string(),
            "https://example.com/app.js".to_string(),
            "https://example.com/data.json".to_string(),
            "https://example.com/manifest.webmanifest".to_string(),
            "https://example.com/logo.png".to_string(),
            "https://example.com/photo.jpg".to_string(),
            "https://example.com/icon.svg".to_string(),
            "https://example.com/font.woff2".to_string(),
            "https://example.com/font.ttf".to_string(),
            "https://example.com/song.mp3".to_string(),
            "https://example.com/video.mp4".to_string(),
            "https://example.com/clip.webm".to_string(),
            "https://example.com/report.pdf".to_string(),
            "https://example.com/archive.zip".to_string(),
            "https://example.com/robots.txt".to_string(),
            "https://example.com/data.csv".to_string(),
            "https://example.com/feed.rss".to_string(),
            "https://example.com/feed.atom".to_string(),
            "https://example.com/real-page".to_string(),
        ];
        let results = extract_links(&links);
        assert_eq!(results.len(), 1);
        assert!(results[0].contains("real-page"));
    }

    #[test]
    fn extension_check_ignores_query_string() {
        let links = vec![
            "https://example.com/page?file=style.css".to_string(),
        ];
        assert_eq!(extract_links(&links).len(), 1, "query param with .css should not be blocked");
    }

    #[test]
    fn asset_path_patterns_rejected() {
        let links = vec![
            "https://example.com/_next/static/chunks/main.js".to_string(),
            "https://example.com/static/images/header.png".to_string(),
            "https://example.com/assets/logo.svg".to_string(),
            "https://example.com/dist/bundle.js".to_string(),
            "https://example.com/build/output.css".to_string(),
            "https://example.com/wp-includes/js/jquery.js".to_string(),
            "https://example.com/wp-content/plugins/akismet/readme.txt".to_string(),
            "https://example.com/wp-content/themes/flavor/style.css".to_string(),
            "https://example.com/cdn-cgi/l/email-protection".to_string(),
            "https://example.com/wp-json/wp/v2/posts".to_string(),
            "https://example.com/xmlrpc.php".to_string(),
            "https://example.com/cgi-bin/script.pl".to_string(),
            "https://example.com/real-page".to_string(),
        ];
        let results = extract_links(&links);
        assert_eq!(results.len(), 1);
        assert!(results[0].contains("real-page"));
    }

    #[test]
    fn percent_encoded_paths_still_caught() {
        let links = vec![
            "https://example.com/%2Fstatic%2Fmain.js".to_string(),
            "https://example.com/_next%2Fstatic%2Fchunk.js".to_string(),
        ];
        assert!(extract_links(&links).is_empty());
    }

    // -----------------------------------------------------------------------
    // Pass 3 — Infrastructure host gate
    // -----------------------------------------------------------------------

    #[test]
    fn infra_subdomain_prefixes_rejected() {
        let links = vec![
            "https://assets.example.com/page".to_string(),
            "https://static.example.com/page".to_string(),
            "https://cdn.example.com/page".to_string(),
            "https://api.example.com/v1/data".to_string(),
            "https://fonts.example.com/roboto".to_string(),
            "https://analytics.example.com/dashboard".to_string(),
            "https://accounts.example.com/login".to_string(),
            "https://login.example.com/sso".to_string(),
            "https://auth.example.com/oauth".to_string(),
            "https://example.com/real-page".to_string(),
        ];
        let results = extract_links(&links);
        assert_eq!(results.len(), 1);
        assert!(results[0].contains("real-page"));
    }

    #[test]
    fn infra_domains_rejected() {
        let links = vec![
            "https://fonts.googleapis.com/css".to_string(),
            "https://www.gstatic.com/images/branding".to_string(),
            "https://www.googletagmanager.com/gtag/js".to_string(),
            "https://cloudflare.com/cdn-cgi/something".to_string(),
            "https://cdn.jsdelivr.net/npm/bootstrap@5".to_string(),
            "https://schema.org/Organization".to_string(),
            "https://www.w3.org/TR/html5/".to_string(),
            "https://purl.org/dc/elements/1.1/".to_string(),
            "https://sentry.io/for/javascript/".to_string(),
            "https://localcommunity.org/events".to_string(),
        ];
        let results = extract_links(&links);
        assert_eq!(results.len(), 1);
        assert!(results[0].contains("localcommunity.org"));
    }

    #[test]
    fn infra_domain_match_is_exact_not_substring() {
        let links = vec![
            "https://marketsegment.com/community".to_string(),
            "https://theschema.org/about".to_string(),
        ];
        let results = extract_links(&links);
        assert_eq!(results.len(), 2, "substring matches must not trigger infra domain block");
    }

    #[test]
    fn extract_host_strips_www_and_port() {
        assert_eq!(extract_host("https://www.google.com/path"), Some("google.com"));
        assert_eq!(extract_host("http://example.com:8080/path"), Some("example.com"));
        assert_eq!(extract_host("https://sub.google.com/"), Some("sub.google.com"));
    }

    // -----------------------------------------------------------------------
    // Semantic URLs now pass through (LLM filter decides)
    // -----------------------------------------------------------------------

    #[test]
    fn major_platforms_pass_through_to_llm_filter() {
        let links = vec![
            "https://www.google.com/maps/place/Community+Center".to_string(),
            "https://youtube.com/watch?v=abc123".to_string(),
            "https://facebook.com/localgroup".to_string(),
            "https://wikipedia.org/wiki/My_Town".to_string(),
            "https://wordpress.com/my-community-blog".to_string(),
        ];
        assert_eq!(extract_links(&links).len(), 5);
    }

    #[test]
    fn privacy_and_legal_pages_pass_through_to_llm_filter() {
        let links = vec![
            "https://example.com/privacy".to_string(),
            "https://example.com/terms".to_string(),
            "https://example.com/legal/policy".to_string(),
        ];
        assert_eq!(extract_links(&links).len(), 3);
    }

    // -----------------------------------------------------------------------
    // Integration — real Linktree-style page
    // -----------------------------------------------------------------------

    #[test]
    fn linktree_page_junk_filtered_content_preserved() {
        let links = vec![
            // Kept: real community sources
            "https://instagram.com/mutual_aid_mpls".to_string(),
            "https://x.com/mpls_aid".to_string(),
            "https://gofundme.com/f/help-my-family".to_string(),
            "https://www.eventbrite.com/e/community-dinner-123".to_string(),
            "https://anotherorg.org/resources".to_string(),
            "https://docs.google.com/document/d/1abc/edit".to_string(),
            // Rejected: scheme
            "mailto:contact@org.com".to_string(),
            // Rejected: static asset
            "https://example.com/flyer.pdf".to_string(),
            "https://example.com/manifest.webmanifest".to_string(),
            "https://example.com/style.css".to_string(),
            // Rejected: infra host
            "https://fonts.googleapis.com/css2?family=Inter".to_string(),
            "https://cdn.example.com/image.png".to_string(),
            "https://accounts.google.com/signin".to_string(),
        ];
        let results = extract_links(&links);
        let urls: Vec<&str> = results.iter().map(|s| s.as_str()).collect();

        assert!(urls.iter().any(|u| u.contains("instagram.com")));
        assert!(urls.iter().any(|u| u.contains("x.com")));
        assert!(urls.iter().any(|u| u.contains("gofundme.com")));
        assert!(urls.iter().any(|u| u.contains("eventbrite.com")));
        assert!(urls.iter().any(|u| u.contains("anotherorg.org")));
        assert!(urls.iter().any(|u| u.contains("docs.google.com")));
        assert_eq!(results.len(), 6);
    }

    // -----------------------------------------------------------------------
    // Sanitize + dedup
    // -----------------------------------------------------------------------

    #[test]
    fn tracking_params_stripped() {
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
    fn tracking_only_query_string_removed_entirely() {
        let links = vec!["https://example.com/page?utm_source=ig&fbclid=abc".to_string()];
        let results = extract_links(&links);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], "https://example.com/page");
    }

    #[test]
    fn duplicate_urls_with_different_tracking_collapsed() {
        let links = vec![
            "https://example.com/page?utm_source=ig".to_string(),
            "https://example.com/page?utm_source=twitter".to_string(),
            "https://example.com/page".to_string(),
        ];
        assert_eq!(extract_links(&links).len(), 1);
    }

    // -----------------------------------------------------------------------
    // canonical_value vs sanitize_url — documenting the distinction
    // -----------------------------------------------------------------------

    #[test]
    fn canonical_value_preserves_tracking_params_sanitize_url_strips_them() {
        use crate::infra::util::sanitize_url;

        let url = "https://example.com/page?utm_source=ig&si=abc&important=yes";

        let cv = canonical_value(url);
        let sanitized = sanitize_url(url);

        assert!(cv.contains("utm_source"), "canonical_value preserves tracking params");
        assert!(cv.contains("si="), "canonical_value preserves si param");
        assert!(!sanitized.contains("utm_source"), "sanitize_url strips utm params");
        assert!(!sanitized.contains("si="), "sanitize_url strips si param");
        assert!(sanitized.contains("important=yes"), "sanitize_url keeps non-tracking params");
    }
}

