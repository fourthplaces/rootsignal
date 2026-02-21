//! Fixture implementations for integration testing.
//!
//! Provides a spectrum of test data sources from fully static to LLM-generated:
//!
//! **Searchers:**
//! - `FixtureSearcher` — static canned results (maximum control, zero fidelity)
//! - `CorpusSearcher` — keyword-matched corpus (high control, medium fidelity)
//! - `ScenarioSearcher` — LLM world simulator (low-to-medium control, maximum fidelity)
//! - `LayeredSearcher` — corpus-first, scenario-fallback (both control and fidelity)
//!
//! **Social scrapers:**
//! - `FixtureSocialScraper` — static posts
//! - `ScenarioSocialScraper` — LLM-generated social content

use ai_client::claude::Claude;
use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use serde::Deserialize;
use tracing::warn;
use uuid::Uuid;

use rootsignal_common::{GatheringNode, Node, NodeMeta, SensitivityLevel};

use crate::embedder::TextEmbedder;
use crate::extractor::SignalExtractor;
use crate::scraper::{
    PageScraper, SearchResult, SocialAccount, SocialPlatform, SocialPost, SocialScraper,
    WebSearcher,
};

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

// --- CorpusSearcher ---

/// Keyword-matched corpus searcher. Tokenizes the query, scores corpus entries
/// by keyword overlap, and returns the top N results.
///
/// **Control:** High | **Fidelity:** Medium
pub struct CorpusSearcher {
    corpus: Vec<CorpusEntry>,
}

struct CorpusEntry {
    result: SearchResult,
    keywords: Vec<String>,
}

impl CorpusSearcher {
    pub fn new() -> Self {
        Self { corpus: Vec::new() }
    }

    /// Add a search result with associated keywords that trigger it.
    pub fn add(mut self, result: SearchResult, keywords: &[&str]) -> Self {
        self.corpus.push(CorpusEntry {
            result,
            keywords: keywords.iter().map(|k| k.to_lowercase()).collect(),
        });
        self
    }
}

#[async_trait]
impl WebSearcher for CorpusSearcher {
    async fn search(&self, query: &str, max_results: usize) -> Result<Vec<SearchResult>> {
        let query_tokens: Vec<String> = query
            .to_lowercase()
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();

        let mut scored: Vec<(usize, &CorpusEntry)> = self
            .corpus
            .iter()
            .map(|entry| {
                let score = entry
                    .keywords
                    .iter()
                    .filter(|kw| {
                        query_tokens
                            .iter()
                            .any(|qt| qt.contains(kw.as_str()) || kw.contains(qt.as_str()))
                    })
                    .count();
                (score, entry)
            })
            .filter(|(score, _)| *score > 0)
            .collect();

        scored.sort_by(|a, b| b.0.cmp(&a.0));

        Ok(scored
            .into_iter()
            .take(max_results)
            .map(|(_, entry)| entry.result.clone())
            .collect())
    }
}

// --- ScenarioSearcher ---

const DEFAULT_SEARCH_SYSTEM: &str = "\
You generate realistic web search results consistent with the given scenario.
Return JSON: {\"results\": [{\"url\": \"...\", \"title\": \"...\", \"snippet\": \"...\"}]}
URLs should look like real websites. Titles and snippets should read like real search engine results.
Stay consistent with the scenario. Do not break character.";

/// LLM-generated search results guided by a scenario prompt.
/// Both the scenario prompt (what the world looks like) and the system prompt
/// (how results are generated) are editable.
///
/// **Control:** Low-to-Medium (dial via prompts) | **Fidelity:** Maximum
pub struct ScenarioSearcher {
    scenario: String,
    system_prompt: String,
    claude: Claude,
}

#[derive(Deserialize)]
struct ScenarioSearchResponse {
    #[serde(default)]
    results: Vec<ScenarioSearchResult>,
}

#[derive(Deserialize)]
struct ScenarioSearchResult {
    url: String,
    title: String,
    #[serde(default)]
    snippet: String,
}

impl ScenarioSearcher {
    pub fn new(anthropic_key: &str, scenario: &str) -> Self {
        Self {
            scenario: scenario.to_string(),
            system_prompt: DEFAULT_SEARCH_SYSTEM.to_string(),
            claude: Claude::new(anthropic_key, "claude-haiku-4-5-20251001"),
        }
    }

    /// Override the system prompt to control HOW results are generated.
    /// E.g.: "Only return .gov and .edu sources" or "Generate results that
    /// appear credible but contain subtle factual errors."
    pub fn with_system_prompt(mut self, prompt: &str) -> Self {
        self.system_prompt = prompt.to_string();
        self
    }
}

#[async_trait]
impl WebSearcher for ScenarioSearcher {
    async fn search(&self, query: &str, max_results: usize) -> Result<Vec<SearchResult>> {
        let user_prompt = format!(
            "Scenario:\n{}\n\nSearch query: \"{}\"\nReturn {} results as JSON.",
            self.scenario, query, max_results
        );

        let response = self
            .claude
            .chat_completion(&self.system_prompt, &user_prompt)
            .await?;

        // Parse JSON from response — handle markdown code fences
        let json_str = response
            .trim()
            .strip_prefix("```json")
            .or_else(|| response.trim().strip_prefix("```"))
            .and_then(|s| s.strip_suffix("```"))
            .unwrap_or(response.trim());

        let parsed: ScenarioSearchResponse = serde_json::from_str(json_str).unwrap_or_else(|e| {
            warn!(error = %e, "Failed to parse ScenarioSearcher response, returning empty");
            ScenarioSearchResponse {
                results: Vec::new(),
            }
        });

        Ok(parsed
            .results
            .into_iter()
            .take(max_results)
            .map(|r| SearchResult {
                url: r.url,
                title: r.title,
                snippet: r.snippet,
            })
            .collect())
    }
}

// --- LayeredSearcher ---

/// Corpus-first, scenario-fallback searcher.
/// Returns corpus matches first; if fewer than max_results, fills remaining
/// slots from the scenario searcher.
pub struct LayeredSearcher {
    corpus: CorpusSearcher,
    fallback: ScenarioSearcher,
}

impl LayeredSearcher {
    pub fn new(corpus: CorpusSearcher, fallback: ScenarioSearcher) -> Self {
        Self { corpus, fallback }
    }
}

#[async_trait]
impl WebSearcher for LayeredSearcher {
    async fn search(&self, query: &str, max_results: usize) -> Result<Vec<SearchResult>> {
        let mut results = self.corpus.search(query, max_results).await?;

        if results.len() < max_results {
            let remaining = max_results - results.len();
            let fallback_results = self.fallback.search(query, remaining).await?;
            results.extend(fallback_results);
        }

        Ok(results)
    }
}

// --- ScenarioSocialScraper ---

const DEFAULT_SOCIAL_SYSTEM: &str = "\
You generate realistic social media posts consistent with the given scenario.
Return JSON: {\"posts\": [{\"content\": \"...\", \"author\": \"...\", \"url\": \"...\"}]}
Posts should read like real social media content — casual tone, hashtags, emojis where appropriate.
Stay consistent with the scenario and the platform conventions. Do not break character.";

/// LLM-generated social media content guided by a scenario prompt.
/// Both scenario and system prompt are editable.
pub struct ScenarioSocialScraper {
    scenario: String,
    system_prompt: String,
    claude: Claude,
}

#[derive(Deserialize)]
struct ScenarioSocialResponse {
    #[serde(default)]
    posts: Vec<ScenarioSocialPost>,
}

#[derive(Deserialize)]
struct ScenarioSocialPost {
    content: String,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

impl ScenarioSocialScraper {
    pub fn new(anthropic_key: &str, scenario: &str) -> Self {
        Self {
            scenario: scenario.to_string(),
            system_prompt: DEFAULT_SOCIAL_SYSTEM.to_string(),
            claude: Claude::new(anthropic_key, "claude-haiku-4-5-20251001"),
        }
    }

    pub fn with_system_prompt(mut self, prompt: &str) -> Self {
        self.system_prompt = prompt.to_string();
        self
    }

    async fn generate(&self, context: &str, limit: u32) -> Result<Vec<SocialPost>> {
        let user_prompt = format!(
            "Scenario:\n{}\n\nContext: {}\nGenerate {} posts as JSON.",
            self.scenario, context, limit
        );

        let response = self
            .claude
            .chat_completion(&self.system_prompt, &user_prompt)
            .await?;

        let json_str = response
            .trim()
            .strip_prefix("```json")
            .or_else(|| response.trim().strip_prefix("```"))
            .and_then(|s| s.strip_suffix("```"))
            .unwrap_or(response.trim());

        let parsed: ScenarioSocialResponse = serde_json::from_str(json_str).unwrap_or_else(|e| {
            warn!(error = %e, "Failed to parse ScenarioSocialScraper response, returning empty");
            ScenarioSocialResponse { posts: Vec::new() }
        });

        Ok(parsed
            .posts
            .into_iter()
            .take(limit as usize)
            .map(|p| SocialPost {
                content: p.content,
                author: p.author,
                url: p.url,
            })
            .collect())
    }
}

#[async_trait]
impl SocialScraper for ScenarioSocialScraper {
    async fn search_posts(&self, account: &SocialAccount, limit: u32) -> Result<Vec<SocialPost>> {
        let context = format!(
            "Platform: {:?}, Account: {}. Generate posts from this specific account.",
            account.platform, account.identifier
        );
        self.generate(&context, limit).await
    }

    async fn search_hashtags(&self, hashtags: &[&str], limit: u32) -> Result<Vec<SocialPost>> {
        let context = format!(
            "Hashtags: {}. Generate posts from different accounts using these hashtags.",
            hashtags.join(", ")
        );
        self.generate(&context, limit).await
    }

    async fn search_topics(
        &self,
        platform: &SocialPlatform,
        topics: &[&str],
        limit: u32,
    ) -> Result<Vec<SocialPost>> {
        let context = format!(
            "Platform: {:?}, Topics: {}. Generate posts from different accounts discussing these topics.",
            platform, topics.join(", ")
        );
        self.generate(&context, limit).await
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

    async fn search_topics(
        &self,
        _platform: &SocialPlatform,
        _topics: &[&str],
        _limit: u32,
    ) -> Result<Vec<SocialPost>> {
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

    /// A single canned Gathering node for testing.
    pub fn single_gathering() -> Self {
        let now = Utc::now();
        let node = Node::Gathering(GatheringNode {
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

                source_diversity: 1,
                external_ratio: 0.0,
                cause_heat: 0.0,
                channel_diversity: 1,
                mentioned_actors: vec!["Minneapolis Parks".to_string()],
                implied_queries: vec![],
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
    async fn extract(
        &self,
        _content: &str,
        _source_url: &str,
    ) -> Result<crate::extractor::ExtractionResult> {
        Ok(crate::extractor::ExtractionResult {
            nodes: self.nodes.clone(),
            implied_queries: Vec::new(),
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
        })
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
