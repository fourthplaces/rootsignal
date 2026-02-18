//! Thin adapters wrapping SimulatedWeb to implement Scout's traits.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use rootsignal_scout::scraper::{
    PageScraper, SearchResult, SocialAccount, SocialPost, SocialScraper, WebSearcher,
};
use simweb::SimulatedWeb;

/// Adapts SimulatedWeb search to the WebSearcher trait.
pub struct SimSearchAdapter {
    sim: Arc<SimulatedWeb>,
}

impl SimSearchAdapter {
    pub fn new(sim: Arc<SimulatedWeb>) -> Self {
        Self { sim }
    }
}

#[async_trait]
impl WebSearcher for SimSearchAdapter {
    async fn search(&self, query: &str, max_results: usize) -> Result<Vec<SearchResult>> {
        let results = self.sim.search(query, max_results).await?;
        Ok(results
            .into_iter()
            .map(|r| SearchResult {
                url: r.url,
                title: r.title,
                snippet: r.snippet,
            })
            .collect())
    }
}

/// Adapts SimulatedWeb scrape to the PageScraper trait.
pub struct SimPageAdapter {
    sim: Arc<SimulatedWeb>,
}

impl SimPageAdapter {
    pub fn new(sim: Arc<SimulatedWeb>) -> Self {
        Self { sim }
    }
}

#[async_trait]
impl PageScraper for SimPageAdapter {
    async fn scrape(&self, url: &str) -> Result<String> {
        let page = self.sim.scrape(url).await?;
        Ok(page.content)
    }

    fn name(&self) -> &str {
        "simweb"
    }
}

/// Adapts SimulatedWeb social methods to the SocialScraper trait.
pub struct SimSocialAdapter {
    sim: Arc<SimulatedWeb>,
}

impl SimSocialAdapter {
    pub fn new(sim: Arc<SimulatedWeb>) -> Self {
        Self { sim }
    }
}

#[async_trait]
impl SocialScraper for SimSocialAdapter {
    async fn search_posts(
        &self,
        account: &SocialAccount,
        limit: u32,
    ) -> Result<Vec<SocialPost>> {
        let platform = format!("{:?}", account.platform);
        let posts = self
            .sim
            .social_posts(&platform, &account.identifier, limit)
            .await?;
        Ok(posts
            .into_iter()
            .map(|p| SocialPost {
                content: p.content,
                author: p.author,
                url: p.url,
            })
            .collect())
    }

    async fn search_hashtags(&self, hashtags: &[&str], limit: u32) -> Result<Vec<SocialPost>> {
        let owned: Vec<String> = hashtags.iter().map(|h| h.to_string()).collect();
        let posts = self.sim.social_hashtags(&owned, limit).await?;
        Ok(posts
            .into_iter()
            .map(|p| SocialPost {
                content: p.content,
                author: p.author,
                url: p.url,
            })
            .collect())
    }
}
