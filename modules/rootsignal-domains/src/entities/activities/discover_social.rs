//! Discover social media profiles from an entity's crawled page snapshots.
//!
//! Scans HTML/markdown stored in `page_snapshots` (linked via `domain_snapshots`)
//! for social media profile URLs, then auto-creates source + social_source records.

use anyhow::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::HashSet;
use std::sync::LazyLock;
use tracing::{info, warn};
use uuid::Uuid;

use crate::scraping::Source;

// =============================================================================
// Types
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedSocialLink {
    pub platform: String,
    pub handle: String,
    pub url: Option<String>,
}

// =============================================================================
// Regex Patterns
// =============================================================================

static RE_INSTAGRAM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:https?://)?(?:www\.)?instagram\.com/([A-Za-z0-9_.]+)").unwrap()
});
static RE_FACEBOOK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:https?://)?(?:www\.)?facebook\.com/([A-Za-z0-9_.]+)").unwrap()
});
static RE_TWITTER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:https?://)?(?:www\.)?(?:twitter|x)\.com/([A-Za-z0-9_]+)").unwrap()
});
static RE_TIKTOK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:https?://)?(?:www\.)?tiktok\.com/@([A-Za-z0-9_.]+)").unwrap());
static RE_LINKEDIN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:https?://)?(?:www\.)?linkedin\.com/company/([A-Za-z0-9_-]+)").unwrap()
});
static RE_YOUTUBE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:https?://)?(?:www\.)?youtube\.com/(?:@|c/|channel/)([A-Za-z0-9_-]+)").unwrap()
});

// =============================================================================
// Skip Segments (non-profile paths to filter out)
// =============================================================================

const INSTAGRAM_SKIP: &[&str] = &[
    "p", "reel", "reels", "stories", "explore", "accounts", "tv", "s", "share",
];
const FACEBOOK_SKIP: &[&str] = &[
    "photo",
    "photos",
    "sharer",
    "share",
    "events",
    "groups",
    "watch",
    "marketplace",
    "login",
    "dialog",
    "plugins",
];
const TWITTER_SKIP: &[&str] = &["intent", "share", "hashtag", "search", "i", "home"];
const TIKTOK_SKIP: &[&str] = &["discover", "tag", "music", "sound"];
const LINKEDIN_SKIP: &[&str] = &["login", "feed", "jobs"];
const YOUTUBE_SKIP: &[&str] = &["watch", "playlist", "results", "feed"];

struct SocialPattern {
    platform: &'static str,
    regex: &'static LazyLock<Regex>,
    skip_segments: &'static [&'static str],
}

const SOCIAL_PATTERNS: &[SocialPattern] = &[
    SocialPattern {
        platform: "instagram",
        regex: &RE_INSTAGRAM,
        skip_segments: INSTAGRAM_SKIP,
    },
    SocialPattern {
        platform: "facebook",
        regex: &RE_FACEBOOK,
        skip_segments: FACEBOOK_SKIP,
    },
    SocialPattern {
        platform: "twitter",
        regex: &RE_TWITTER,
        skip_segments: TWITTER_SKIP,
    },
    SocialPattern {
        platform: "tiktok",
        regex: &RE_TIKTOK,
        skip_segments: TIKTOK_SKIP,
    },
    SocialPattern {
        platform: "linkedin",
        regex: &RE_LINKEDIN,
        skip_segments: LINKEDIN_SKIP,
    },
    SocialPattern {
        platform: "youtube",
        regex: &RE_YOUTUBE,
        skip_segments: YOUTUBE_SKIP,
    },
];

// =============================================================================
// Scanning
// =============================================================================

/// Scan content for social media profile URLs using regex.
/// Returns deduplicated profiles found across all content.
pub fn scan_social_links(content: &str) -> Vec<ExtractedSocialLink> {
    let mut seen = HashSet::new();
    let mut profiles = Vec::new();

    for pattern in SOCIAL_PATTERNS {
        for cap in pattern.regex.captures_iter(content) {
            let handle = cap[1].to_lowercase();

            // Skip non-profile path segments
            if pattern.skip_segments.contains(&handle.as_str()) {
                continue;
            }

            let key = (pattern.platform.to_string(), handle.clone());
            if seen.insert(key) {
                let url = cap.get(0).map(|m| m.as_str().to_string());

                profiles.push(ExtractedSocialLink {
                    platform: pattern.platform.to_string(),
                    handle,
                    url,
                });
            }
        }
    }

    profiles
}

// =============================================================================
// Entity-Level Discovery
// =============================================================================

/// Discover social media profiles from all page snapshots belonging to an entity's sources.
///
/// 1. Load HTML/markdown from page_snapshots linked to the entity's sources
/// 2. Scan for social media profile URLs
/// 3. Create source + social_source records for each discovered profile
pub async fn discover_social_for_entity(entity_id: Uuid, pool: &PgPool) -> Result<Vec<Source>> {
    // Load all page content from snapshots linked to this entity's sources
    let rows = sqlx::query_as::<_, (String,)>(
        r#"
        SELECT COALESCE(ps.raw_content, ps.html, '')
        FROM page_snapshots ps
        JOIN domain_snapshots ds ON ds.page_snapshot_id = ps.id
        JOIN sources s ON s.id = ds.source_id
        WHERE s.entity_id = $1
          AND (ps.raw_content IS NOT NULL OR ps.html IS NOT NULL)
        "#,
    )
    .bind(entity_id)
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        info!(entity_id = %entity_id, "No page snapshots found for entity");
        return Ok(vec![]);
    }

    info!(
        entity_id = %entity_id,
        page_count = rows.len(),
        "Scanning page snapshots for social links"
    );

    // Scan all pages for social links
    let mut all_links = Vec::new();
    let mut seen = HashSet::new();

    for (content,) in &rows {
        for link in scan_social_links(content) {
            let key = (link.platform.clone(), link.handle.clone());
            if seen.insert(key) {
                all_links.push(link);
            }
        }
    }

    info!(
        entity_id = %entity_id,
        found = all_links.len(),
        "Social links discovered"
    );

    // Create source + social_source for each discovered link
    let mut created_sources = Vec::new();

    for link in &all_links {
        match Source::find_or_create_social(
            &link.platform,
            &link.handle,
            link.url.as_deref(),
            entity_id,
            pool,
        )
        .await
        {
            Ok((source, was_created)) => {
                if was_created {
                    info!(
                        entity_id = %entity_id,
                        platform = %link.platform,
                        handle = %link.handle,
                        source_id = %source.id,
                        "Created social source"
                    );
                }
                created_sources.push(source);
            }
            Err(e) => {
                warn!(
                    entity_id = %entity_id,
                    platform = %link.platform,
                    handle = %link.handle,
                    error = %e,
                    "Failed to create social source, continuing"
                );
            }
        }
    }

    Ok(created_sources)
}

// =============================================================================
// URL-Based Discovery
// =============================================================================

/// Fetch a URL via HTTP and scan the HTML for social media profile links.
///
/// Best-effort: logs warnings on HTTP errors, returns `Ok(vec![])` on failure.
/// Uses a 15-second timeout to avoid blocking.
pub async fn discover_social_from_url(
    url: &str,
    entity_id: Uuid,
    client: &reqwest::Client,
    pool: &PgPool,
) -> Result<Vec<Source>> {
    let response = match client
        .get(url)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            warn!(url = %url, error = %e, "Failed to fetch URL for social discovery");
            return Ok(vec![]);
        }
    };

    let html = match response.text().await {
        Ok(text) => text,
        Err(e) => {
            warn!(url = %url, error = %e, "Failed to read response body for social discovery");
            return Ok(vec![]);
        }
    };

    let links = scan_social_links(&html);

    if links.is_empty() {
        info!(url = %url, "No social links found from URL");
        return Ok(vec![]);
    }

    info!(url = %url, found = links.len(), "Social links found from URL");

    let mut created_sources = Vec::new();

    for link in &links {
        match Source::find_or_create_social(
            &link.platform,
            &link.handle,
            link.url.as_deref(),
            entity_id,
            pool,
        )
        .await
        {
            Ok((source, was_created)) => {
                if was_created {
                    info!(
                        entity_id = %entity_id,
                        platform = %link.platform,
                        handle = %link.handle,
                        source_id = %source.id,
                        "Created social source from URL"
                    );
                }
                created_sources.push(source);
            }
            Err(e) => {
                warn!(
                    entity_id = %entity_id,
                    platform = %link.platform,
                    handle = %link.handle,
                    error = %e,
                    "Failed to create social source from URL, continuing"
                );
            }
        }
    }

    Ok(created_sources)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_social_links_basic() {
        let content = r#"
Follow us on https://www.instagram.com/myhandle and https://facebook.com/mypage
Also on https://x.com/mytwitter
        "#;

        let profiles = scan_social_links(content);
        assert_eq!(profiles.len(), 3);

        let platforms: Vec<&str> = profiles.iter().map(|p| p.platform.as_str()).collect();
        assert!(platforms.contains(&"instagram"));
        assert!(platforms.contains(&"facebook"));
        assert!(platforms.contains(&"twitter"));
    }

    #[test]
    fn test_scan_skips_non_profile_segments() {
        let content = r#"
https://www.instagram.com/communityaidnetworkmn/
https://www.instagram.com/p/DUPMIPgkVkr/
https://www.instagram.com/reel/ABC123/
https://facebook.com/events/123456
        "#;

        let profiles = scan_social_links(content);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].handle, "communityaidnetworkmn");
    }

    #[test]
    fn test_scan_deduplicates() {
        let content = r#"
https://instagram.com/myhandle
https://www.instagram.com/myhandle
        "#;

        let profiles = scan_social_links(content);
        assert_eq!(profiles.len(), 1, "Should deduplicate same handle");
    }

    #[test]
    fn test_scan_markdown_links() {
        let content = r#"
[Follow us](https://www.instagram.com/communityaidnetworkmn/)
[Facebook](https://www.facebook.com/CommunityAidNetworkMN)
[Twitter](https://x.com/canaboretum)
        "#;

        let profiles = scan_social_links(content);
        let platforms: Vec<&str> = profiles.iter().map(|p| p.platform.as_str()).collect();
        let handles: Vec<&str> = profiles.iter().map(|p| p.handle.as_str()).collect();

        assert!(platforms.contains(&"instagram"));
        assert!(platforms.contains(&"facebook"));
        assert!(platforms.contains(&"twitter"));
        assert!(handles.contains(&"communityaidnetworkmn"));
        assert!(handles.contains(&"canaboretum"));
    }

    #[test]
    fn test_scan_tiktok() {
        let content = "Check us out at https://www.tiktok.com/@myhandle";
        let profiles = scan_social_links(content);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].platform, "tiktok");
        assert_eq!(profiles[0].handle, "myhandle");
    }

    #[test]
    fn test_scan_linkedin() {
        let content = "https://www.linkedin.com/company/my-company";
        let profiles = scan_social_links(content);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].platform, "linkedin");
        assert_eq!(profiles[0].handle, "my-company");
    }

    #[test]
    fn test_scan_html_anchor_tags() {
        let content = r#"
<html>
<body>
  <a href="https://www.instagram.com/coffeeshop">Instagram</a>
  <a href="https://www.facebook.com/coffeeshop">Facebook</a>
  <a href="https://x.com/coffeeshop">Twitter</a>
</body>
</html>
        "#;

        let profiles = scan_social_links(content);
        assert_eq!(profiles.len(), 3);

        let platforms: Vec<&str> = profiles.iter().map(|p| p.platform.as_str()).collect();
        assert!(platforms.contains(&"instagram"));
        assert!(platforms.contains(&"facebook"));
        assert!(platforms.contains(&"twitter"));
        assert!(profiles.iter().all(|p| p.handle == "coffeeshop"));
    }

    #[test]
    fn test_scan_html_mixed_attributes() {
        let content = r#"
<a class="social-link" href="https://www.instagram.com/myshop" target="_blank" rel="noopener">
  <img src="instagram-icon.png" />
</a>
<a href="https://www.tiktok.com/@myshop">TikTok</a>
<a href="https://www.linkedin.com/company/my-shop">LinkedIn</a>
        "#;

        let profiles = scan_social_links(content);
        assert_eq!(profiles.len(), 3);

        let platforms: Vec<&str> = profiles.iter().map(|p| p.platform.as_str()).collect();
        assert!(platforms.contains(&"instagram"));
        assert!(platforms.contains(&"tiktok"));
        assert!(platforms.contains(&"linkedin"));
    }

    #[test]
    fn test_scan_youtube() {
        let content = r#"
https://www.youtube.com/@mychannel
https://youtube.com/c/oldchannel
https://youtube.com/channel/UC123abc
        "#;

        let profiles = scan_social_links(content);
        assert_eq!(profiles.len(), 3);
        let platforms: Vec<&str> = profiles.iter().map(|p| p.platform.as_str()).collect();
        assert!(platforms.iter().all(|p| *p == "youtube"));
    }
}
