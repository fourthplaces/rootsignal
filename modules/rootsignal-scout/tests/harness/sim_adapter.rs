//! Thin adapter wrapping SimulatedWeb to implement FetchBackend.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;

use rootsignal_archive::{ArchiveError, Content, FetchBackend, FetchedContent};
use rootsignal_common::{ContentSemantics, ScrapedPage, SearchResult, SocialPost};
use simweb::SimulatedWeb;

/// Adapts SimulatedWeb to the unified FetchBackend trait.
pub struct SimArchive {
    sim: Arc<SimulatedWeb>,
}

impl SimArchive {
    pub fn new(sim: Arc<SimulatedWeb>) -> Self {
        Self { sim }
    }
}

#[async_trait]
impl FetchBackend for SimArchive {
    async fn fetch_content(&self, target: &str) -> rootsignal_archive::Result<FetchedContent> {
        let now = Utc::now();

        // Social identifiers
        if target.starts_with("social:") || target.contains("reddit.com/r/") || target.contains("instagram.com") || target.contains("x.com/") || target.contains("tiktok.com") || target.contains("bluesky.social") {
            // Extract platform + identifier from URL
            let (platform, identifier) = parse_social_target(target);
            let posts = self.sim.social_posts(&platform, &identifier, 20).await
                .map_err(|e| ArchiveError::FetchFailed(e.to_string()))?;
            let common_posts: Vec<SocialPost> = posts.into_iter().map(|p| SocialPost {
                content: p.content,
                author: p.author,
                url: p.url,
            }).collect();
            let text = common_posts.iter().map(|p| p.content.as_str()).collect::<Vec<_>>().join("\n");
            return Ok(FetchedContent {
                target: target.to_string(),
                content: Content::SocialPosts(common_posts),
                content_hash: format!("sim-{}", target),
                fetched_at: now,
                duration_ms: 0,
                text,
            });
        }

        // Non-URL targets → search
        if !target.starts_with("http") {
            let results = self.sim.search(target, 10).await
                .map_err(|e| ArchiveError::FetchFailed(e.to_string()))?;
            let common_results: Vec<SearchResult> = results.into_iter().map(|r| SearchResult {
                url: r.url,
                title: r.title,
                snippet: r.snippet,
            }).collect();
            let text = common_results.iter().map(|r| format!("{}: {}", r.title, r.snippet)).collect::<Vec<_>>().join("\n");
            return Ok(FetchedContent {
                target: target.to_string(),
                content: Content::SearchResults(common_results),
                content_hash: format!("sim-{}", target),
                fetched_at: now,
                duration_ms: 0,
                text,
            });
        }

        // URL → scrape page
        let page = self.sim.scrape(target).await
            .map_err(|e| ArchiveError::FetchFailed(e.to_string()))?;
        Ok(FetchedContent {
            target: target.to_string(),
            content: Content::Page(ScrapedPage {
                url: target.to_string(),
                markdown: page.content.clone(),
                raw_html: format!("<html><body>{}</body></html>", page.content),
                content_hash: format!("sim-{}", target),
            }),
            content_hash: format!("sim-{}", target),
            fetched_at: now,
            duration_ms: 0,
            text: page.content,
        })
    }

    async fn resolve_semantics(&self, _content: &FetchedContent) -> rootsignal_archive::Result<ContentSemantics> {
        Err(ArchiveError::Other(anyhow::anyhow!("SimArchive does not support semantics")))
    }
}

fn parse_social_target(target: &str) -> (String, String) {
    if target.contains("reddit.com/r/") {
        let id = target.split("/r/").last().unwrap_or("").trim_end_matches('/');
        return ("Reddit".to_string(), id.to_string());
    }
    if target.contains("instagram.com") {
        let id = target.split("instagram.com/").last().unwrap_or("").trim_end_matches('/');
        return ("Instagram".to_string(), id.to_string());
    }
    if target.contains("x.com/") {
        let id = target.split("x.com/").last().unwrap_or("").trim_end_matches('/');
        return ("Twitter".to_string(), id.to_string());
    }
    if target.contains("tiktok.com") {
        let id = target.split("tiktok.com/@").last().unwrap_or("").trim_end_matches('/');
        return ("TikTok".to_string(), id.to_string());
    }
    // Fallback
    ("Unknown".to_string(), target.to_string())
}
