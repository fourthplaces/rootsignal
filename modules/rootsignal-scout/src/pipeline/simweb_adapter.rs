// Thin adapter: SimulatedWeb implements ContentFetcher.
// Maps simweb's domain-agnostic types to rootsignal's archive types.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use chrono::Utc;
use uuid::Uuid;

use rootsignal_common::types::{
    ArchivedFeed, ArchivedPage, ArchivedSearchResults, Post, SearchResult,
};
use simweb::SimulatedWeb;

use super::traits::ContentFetcher;

#[async_trait]
impl ContentFetcher for SimulatedWeb {
    async fn page(&self, url: &str) -> Result<ArchivedPage> {
        let sim = self.scrape(url).await?;

        // SimulatedWeb doesn't generate raw HTML; use Site.links_to for links.
        let links = self
            .world()
            .sites
            .iter()
            .find(|s| s.url == url)
            .map(|s| s.links_to.clone())
            .unwrap_or_default();

        Ok(ArchivedPage {
            id: Uuid::new_v4(),
            source_id: Uuid::new_v4(),
            fetched_at: Utc::now(),
            content_hash: format!("{:x}", rootsignal_common::content_hash(&sim.content)),
            raw_html: String::new(),
            markdown: sim.content,
            title: None,
            links,
            published_at: None,
        })
    }

    async fn feed(&self, _url: &str) -> Result<ArchivedFeed> {
        Err(anyhow!("SimulatedWeb does not support RSS feeds"))
    }

    async fn posts(&self, identifier: &str, limit: u32) -> Result<Vec<Post>> {
        let (platform, handle) = parse_social_url(identifier);
        let sim_posts = self.social_posts(&platform, &handle, limit).await?;

        Ok(sim_posts
            .into_iter()
            .map(|p| Post {
                id: Uuid::new_v4(),
                source_id: Uuid::new_v4(),
                fetched_at: Utc::now(),
                content_hash: format!("{:x}", rootsignal_common::content_hash(&p.content)),
                text: Some(p.content),
                author: p.author,
                location: None,
                engagement: None,
                published_at: None,
                permalink: p.url,
                mentions: Vec::new(),
                hashtags: Vec::new(),
                media_type: None,
                platform_id: None,
                attachments: Vec::new(),
            })
            .collect())
    }

    async fn search(&self, query: &str) -> Result<ArchivedSearchResults> {
        let sim_results = self.search(query, 10).await?;
        Ok(to_archived_search(query, &sim_results))
    }

    async fn search_topics(
        &self,
        _platform_url: &str,
        topics: &[&str],
        limit: u32,
    ) -> Result<Vec<Post>> {
        let hashtags: Vec<String> = topics.iter().map(|t| t.to_string()).collect();
        let sim_posts = self.social_hashtags(&hashtags, limit).await?;

        Ok(sim_posts
            .into_iter()
            .map(|p| Post {
                id: Uuid::new_v4(),
                source_id: Uuid::new_v4(),
                fetched_at: Utc::now(),
                content_hash: format!("{:x}", rootsignal_common::content_hash(&p.content)),
                text: Some(p.content),
                author: p.author,
                location: None,
                engagement: None,
                published_at: None,
                permalink: p.url,
                mentions: Vec::new(),
                hashtags: Vec::new(),
                media_type: None,
                platform_id: None,
                attachments: Vec::new(),
            })
            .collect())
    }

    async fn site_search(&self, query: &str, max_results: usize) -> Result<ArchivedSearchResults> {
        let sim_results = self.search(query, max_results).await?;
        Ok(to_archived_search(query, &sim_results))
    }
}

// --- Helpers ---

fn to_archived_search(
    query: &str,
    sim_results: &[simweb::SimSearchResult],
) -> ArchivedSearchResults {
    let combined = sim_results
        .iter()
        .map(|r| format!("{}{}{}", r.url, r.title, r.snippet))
        .collect::<String>();

    ArchivedSearchResults {
        id: Uuid::new_v4(),
        source_id: Uuid::new_v4(),
        fetched_at: Utc::now(),
        content_hash: format!("{:x}", rootsignal_common::content_hash(&combined)),
        query: query.to_string(),
        results: sim_results
            .iter()
            .map(|r| SearchResult {
                url: r.url.clone(),
                title: r.title.clone(),
                snippet: r.snippet.clone(),
            })
            .collect(),
    }
}

/// Parse a social media URL into (platform, identifier) for SimulatedWeb.
///
/// `fetcher.posts()` receives full URLs like `https://www.instagram.com/handle`.
/// SimulatedWeb expects lowercase platform name + identifier separately.
fn parse_social_url(url: &str) -> (String, String) {
    let lower = url.to_lowercase();

    let platform = if lower.contains("instagram.com") {
        "instagram"
    } else if lower.contains("twitter.com") || lower.contains("x.com") {
        "twitter"
    } else if lower.contains("reddit.com") {
        "reddit"
    } else if lower.contains("facebook.com") {
        "facebook"
    } else if lower.contains("tiktok.com") {
        "tiktok"
    } else if lower.contains("bsky.app") {
        "bluesky"
    } else {
        "web"
    };

    // Extract the handle/path from the URL.
    // e.g. "https://www.instagram.com/northside_mutual_aid" → "northside_mutual_aid"
    let identifier = url
        .split('/')
        .filter(|seg| !seg.is_empty())
        .last()
        .unwrap_or(url)
        .to_string();

    (platform.to_string(), identifier)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_instagram_url() {
        let (platform, id) = parse_social_url("https://www.instagram.com/northside_mutual_aid");
        assert_eq!(platform, "instagram");
        assert_eq!(id, "northside_mutual_aid");
    }

    #[test]
    fn parse_reddit_url() {
        let (platform, id) = parse_social_url("https://www.reddit.com/r/Minneapolis/");
        assert_eq!(platform, "reddit");
        // Trailing slash gets filtered by the empty-segment filter
        assert_eq!(id, "Minneapolis");
    }

    #[test]
    fn parse_twitter_url() {
        let (platform, id) = parse_social_url("https://x.com/mpaborhood");
        assert_eq!(platform, "twitter");
        assert_eq!(id, "mpaborhood");
    }

    #[test]
    fn parse_bluesky_url() {
        let (platform, id) = parse_social_url("https://bsky.app/profile/community.bsky.social");
        assert_eq!(platform, "bluesky");
        assert_eq!(id, "community.bsky.social");
    }
}
