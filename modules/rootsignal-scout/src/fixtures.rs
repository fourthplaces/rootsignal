//! Fixture implementations for integration testing.
//! Each struct returns canned data so tests run without external API calls.

use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use uuid::Uuid;

use rootsignal_common::{
    AudienceRole, EventNode, Node, NodeMeta, SensitivityLevel,
};

use crate::embedder::TextEmbedder;
use crate::extractor::SignalExtractor;
use crate::scraper::{PageScraper, SearchResult, SocialAccount, SocialPost, SocialScraper, WebSearcher};

// --- FixtureSearcher ---

pub struct FixtureSearcher {
    pub results: Vec<SearchResult>,
}

impl FixtureSearcher {
    pub fn new(results: Vec<SearchResult>) -> Self {
        Self { results }
    }
}

#[async_trait]
impl WebSearcher for FixtureSearcher {
    async fn search(&self, _query: &str, _max_results: usize) -> Result<Vec<SearchResult>> {
        Ok(self.results.clone())
    }
}

// --- FixtureSocialScraper ---

pub struct FixtureSocialScraper {
    pub posts: Vec<SocialPost>,
}

impl FixtureSocialScraper {
    pub fn new(posts: Vec<SocialPost>) -> Self {
        Self { posts }
    }

    pub fn empty() -> Self {
        Self { posts: Vec::new() }
    }
}

#[async_trait]
impl SocialScraper for FixtureSocialScraper {
    async fn search_posts(&self, _account: &SocialAccount, _limit: u32) -> Result<Vec<SocialPost>> {
        Ok(self.posts.clone())
    }

    async fn search_hashtags(&self, _hashtags: &[&str], _limit: u32) -> Result<Vec<SocialPost>> {
        Ok(self.posts.clone())
    }
}

// --- FixtureExtractor ---

pub struct FixtureExtractor {
    pub nodes: Vec<Node>,
}

impl FixtureExtractor {
    pub fn new(nodes: Vec<Node>) -> Self {
        Self { nodes }
    }

    /// A single canned Event node for testing.
    pub fn single_event() -> Self {
        let now = Utc::now();
        let node = Node::Event(EventNode {
            meta: NodeMeta {
                id: Uuid::new_v4(),
                title: "Community Garden Volunteer Day".to_string(),
                summary: "Join neighbors for spring planting at the community garden.".to_string(),
                sensitivity: SensitivityLevel::General,
                confidence: 0.0,
                freshness_score: 1.0,
                corroboration_count: 0,
                location: None,
                location_name: Some("Minneapolis Community Garden".to_string()),
                source_url: String::new(), // set by store_signals
                extracted_at: now,
                last_confirmed_active: now,
                audience_roles: vec![AudienceRole::Volunteer, AudienceRole::Neighbor],
                source_diversity: 1,
                external_ratio: 0.0,
                mentioned_actors: vec!["Minneapolis Parks".to_string()],
            },
            starts_at: Some(now + chrono::Duration::days(7)),
            ends_at: None,
            action_url: "https://example.com/garden".to_string(),
            organizer: Some("Minneapolis Parks".to_string()),
            is_recurring: false,
        });
        Self { nodes: vec![node] }
    }
}

#[async_trait]
impl SignalExtractor for FixtureExtractor {
    async fn extract(&self, _content: &str, _source_url: &str) -> Result<Vec<Node>> {
        Ok(self.nodes.clone())
    }
}

// --- FixtureEmbedder ---

/// Returns deterministic hash-based 1024-dim vectors.
pub struct FixtureEmbedder;

#[async_trait]
impl TextEmbedder for FixtureEmbedder {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        Ok(deterministic_embedding(text))
    }

    async fn embed_batch(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|t| deterministic_embedding(t)).collect())
    }
}

fn deterministic_embedding(text: &str) -> Vec<f32> {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in text.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }

    (0..1024)
        .map(|i| {
            // Mix hash with index to get per-dimension value
            let mixed = hash.wrapping_add(i as u64).wrapping_mul(0x517cc1b727220a95);
            // Normalize to [-1, 1]
            (mixed as i64 as f64 / i64::MAX as f64) as f32
        })
        .collect()
}

// --- FixtureScraper ---

pub struct FixtureScraper {
    pub content: String,
}

impl FixtureScraper {
    pub fn new(content: &str) -> Self {
        Self {
            content: content.to_string(),
        }
    }
}

#[async_trait]
impl PageScraper for FixtureScraper {
    async fn scrape(&self, _url: &str) -> Result<String> {
        Ok(self.content.clone())
    }

    fn name(&self) -> &str {
        "fixture"
    }
}
