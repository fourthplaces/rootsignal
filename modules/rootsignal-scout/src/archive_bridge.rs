// Bridge between rootsignal-archive and scout's scraper traits.
//
// Implements PageScraper, WebSearcher, and SocialScraper on Archive so the
// scout's existing trait-based pipeline records every web interaction through
// the archive. This is a transitional layer — once the scout is fully migrated
// to use Archive directly, the scraper traits can be removed.

use anyhow::Result;
use async_trait::async_trait;

use rootsignal_archive::{Archive, Content};

use crate::pipeline::scraper::{
    PageScraper, SearchResult, SocialAccount, SocialPost, SocialScraper, WebSearcher,
};

#[async_trait]
impl PageScraper for Archive {
    async fn scrape(&self, url: &str) -> Result<String> {
        let response = self.fetch(url).await.map_err(|e| anyhow::anyhow!("{e}"))?;
        match response.content {
            Content::Page(page) => Ok(page.markdown),
            Content::Raw(text) => Ok(text),
            // If the archive detected a feed or something else, return the raw
            // serialized form so the caller has *something* to work with.
            other => Ok(format!("{:?}", other)),
        }
    }

    async fn scrape_raw(&self, url: &str) -> Result<String> {
        let response = self.fetch(url).await.map_err(|e| anyhow::anyhow!("{e}"))?;
        match response.content {
            Content::Page(page) => Ok(page.raw_html),
            Content::Raw(text) => Ok(text),
            other => Ok(format!("{:?}", other)),
        }
    }

    fn name(&self) -> &str {
        "archive"
    }
}

#[async_trait]
impl WebSearcher for Archive {
    async fn search(&self, query: &str, _max_results: usize) -> Result<Vec<SearchResult>> {
        let response = self.fetch(query).await.map_err(|e| anyhow::anyhow!("{e}"))?;
        match response.content {
            Content::SearchResults(results) => Ok(results
                .into_iter()
                .map(|r| SearchResult {
                    url: r.url,
                    title: r.title,
                    snippet: r.snippet,
                })
                .collect()),
            _ => Ok(Vec::new()),
        }
    }
}

#[async_trait]
impl SocialScraper for Archive {
    async fn search_posts(&self, account: &SocialAccount, _limit: u32) -> Result<Vec<SocialPost>> {
        // The archive detects social platform from the URL, so we just pass
        // the identifier (which is the URL for social sources).
        let response = self
            .fetch(&account.identifier)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        match response.content {
            Content::SocialPosts(posts) => Ok(posts
                .into_iter()
                .map(common_to_scout_post)
                .collect()),
            _ => Ok(Vec::new()),
        }
    }

    async fn search_hashtags(&self, hashtags: &[&str], limit: u32) -> Result<Vec<SocialPost>> {
        // Instagram hashtag search through search_social
        let platform = rootsignal_common::SocialPlatform::Instagram;
        let response = self
            .search_social(&platform, hashtags, limit)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        match response.content {
            Content::SocialPosts(posts) => Ok(posts
                .into_iter()
                .map(common_to_scout_post)
                .collect()),
            _ => Ok(Vec::new()),
        }
    }

    async fn search_topics(
        &self,
        platform: &crate::pipeline::scraper::SocialPlatform,
        topics: &[&str],
        limit: u32,
    ) -> Result<Vec<SocialPost>> {
        let common_platform = scout_to_common_platform(platform);
        let response = self
            .search_social(&common_platform, topics, limit)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        match response.content {
            Content::SocialPosts(posts) => Ok(posts
                .into_iter()
                .map(common_to_scout_post)
                .collect()),
            _ => Ok(Vec::new()),
        }
    }
}

fn common_to_scout_post(p: rootsignal_common::SocialPost) -> SocialPost {
    SocialPost {
        content: p.content,
        author: p.author,
        url: p.url,
    }
}

fn scout_to_common_platform(
    p: &crate::pipeline::scraper::SocialPlatform,
) -> rootsignal_common::SocialPlatform {
    match p {
        crate::pipeline::scraper::SocialPlatform::Instagram => {
            rootsignal_common::SocialPlatform::Instagram
        }
        crate::pipeline::scraper::SocialPlatform::Facebook => {
            rootsignal_common::SocialPlatform::Facebook
        }
        crate::pipeline::scraper::SocialPlatform::Reddit => {
            rootsignal_common::SocialPlatform::Reddit
        }
        crate::pipeline::scraper::SocialPlatform::Twitter => {
            rootsignal_common::SocialPlatform::Twitter
        }
        crate::pipeline::scraper::SocialPlatform::TikTok => {
            rootsignal_common::SocialPlatform::TikTok
        }
    }
}
