//! SimulatedWeb — generates coherent web content from a World description.
//!
//! Uses Haiku for all generation. Maintains caches so the same URL/query
//! returns consistent content within a run.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use ai_client::Claude;
use anyhow::Result;
use chrono::Utc;
use serde::Deserialize;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::prompt;
use crate::snapshot::{LogEntry, RunLog};
use crate::types::{SimPage, SimPost, SimSearchResult};
use crate::world::World;

const HAIKU_MODEL: &str = "claude-haiku-4-5-20251001";

/// A simulated web environment backed by LLM generation.
/// All responses are cached for consistency within a run.
pub struct SimulatedWeb {
    world: World,
    claude: Claude,
    search_cache: Arc<RwLock<HashMap<String, Vec<SimSearchResult>>>>,
    page_cache: Arc<RwLock<HashMap<String, SimPage>>>,
    social_cache: Arc<RwLock<HashMap<String, Vec<SimPost>>>>,
    /// Tracks which URLs had snippets generated via search (for scrape consistency).
    snippet_cache: Arc<RwLock<HashMap<String, String>>>,
    log: Arc<RwLock<RunLog>>,
}

#[derive(Deserialize)]
struct SearchResponse {
    #[serde(default)]
    results: Vec<SearchResultWire>,
}

#[derive(Deserialize)]
struct SearchResultWire {
    url: String,
    title: String,
    #[serde(default)]
    snippet: String,
}

#[derive(Deserialize)]
struct SocialResponse {
    #[serde(default)]
    posts: Vec<SocialPostWire>,
}

#[derive(Deserialize)]
struct SocialPostWire {
    content: String,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

impl SimulatedWeb {
    /// Create a new SimulatedWeb from a world description and API key.
    pub fn new(world: World, api_key: &str) -> Self {
        Self {
            world,
            claude: Claude::new(api_key, HAIKU_MODEL),
            search_cache: Arc::new(RwLock::new(HashMap::new())),
            page_cache: Arc::new(RwLock::new(HashMap::new())),
            social_cache: Arc::new(RwLock::new(HashMap::new())),
            snippet_cache: Arc::new(RwLock::new(HashMap::new())),
            log: Arc::new(RwLock::new(RunLog::new())),
        }
    }

    /// Load cached responses from a snapshot for replay (pinned scenarios).
    pub fn from_snapshot(world: World, api_key: &str, path: &Path) -> Result<Self> {
        let run_log = RunLog::load(path)?;
        let mut search_cache = HashMap::new();
        let mut page_cache = HashMap::new();
        let mut social_cache = HashMap::new();
        let mut snippet_cache: HashMap<String, String> = HashMap::new();

        for entry in &run_log.entries {
            match entry {
                LogEntry::Search { query, results, .. } => {
                    // Also populate snippet_cache from search results
                    for r in results {
                        if !r.snippet.is_empty() {
                            snippet_cache.insert(r.url.clone(), r.snippet.clone());
                        }
                    }
                    search_cache.insert(query.clone(), results.clone());
                }
                LogEntry::Scrape { url, page, .. } => {
                    page_cache.insert(url.clone(), page.clone());
                }
                LogEntry::Social {
                    platform,
                    identifier,
                    posts,
                    ..
                } => {
                    let key = format!("{platform}:{identifier}");
                    social_cache.insert(key, posts.clone());
                }
                LogEntry::Hashtags {
                    hashtags, posts, ..
                } => {
                    let key = format!("hashtags:{}", hashtags.join(","));
                    social_cache.insert(key, posts.clone());
                }
            }
        }

        Ok(Self {
            world,
            claude: Claude::new(api_key, HAIKU_MODEL),
            search_cache: Arc::new(RwLock::new(search_cache)),
            page_cache: Arc::new(RwLock::new(page_cache)),
            social_cache: Arc::new(RwLock::new(social_cache)),
            snippet_cache: Arc::new(RwLock::new(snippet_cache)),
            log: Arc::new(RwLock::new(run_log)),
        })
    }

    /// Save all cached responses to disk for replay.
    pub async fn save_snapshot(&self, path: &Path) -> Result<()> {
        let log = self.log.read().await;
        log.save(path)
    }

    /// Simulate a web search. Returns results constrained to world.sites URLs.
    /// Out-of-world queries return an empty vec.
    pub async fn search(&self, query: &str, max_results: usize) -> Result<Vec<SimSearchResult>> {
        // Check cache
        {
            let cache = self.search_cache.read().await;
            if let Some(results) = cache.get(query) {
                return Ok(results.clone());
            }
        }

        let system = prompt::search_system(&self.world);
        let user = prompt::search_user(query, max_results);

        let response = self.claude.chat_completion(&system, &user).await?;
        let results = parse_search_response(&response);

        // Filter to only URLs that exist in world.sites
        let valid_urls: std::collections::HashSet<&str> =
            self.world.sites.iter().map(|s| s.url.as_str()).collect();
        let results: Vec<SimSearchResult> = results
            .into_iter()
            .filter(|r| valid_urls.contains(r.url.as_str()))
            .take(max_results)
            .collect();

        // Update snippet cache for scrape consistency
        {
            let mut snippets = self.snippet_cache.write().await;
            for r in &results {
                if !r.snippet.is_empty() {
                    snippets.insert(r.url.clone(), r.snippet.clone());
                }
            }
        }

        // Cache and log
        {
            let mut cache = self.search_cache.write().await;
            cache.insert(query.to_string(), results.clone());
        }
        {
            let mut log = self.log.write().await;
            log.entries.push(LogEntry::Search {
                query: query.to_string(),
                results: results.clone(),
                timestamp: Utc::now(),
            });
        }

        info!(query, count = results.len(), "SimulatedWeb search");
        Ok(results)
    }

    /// Simulate scraping a URL. Returns page content consistent with the site
    /// description and any prior search snippet for this URL.
    /// Unknown URLs return empty content.
    pub async fn scrape(&self, url: &str) -> Result<SimPage> {
        // Check cache
        {
            let cache = self.page_cache.read().await;
            if let Some(page) = cache.get(url) {
                return Ok(page.clone());
            }
        }

        // Find the site in the world
        let site = self.world.sites.iter().find(|s| s.url == url);
        let site = match site {
            Some(s) => s,
            None => {
                // Unknown URL — return empty content
                let page = SimPage {
                    url: url.to_string(),
                    content: String::new(),
                    raw_html: None,
                };
                let mut cache = self.page_cache.write().await;
                cache.insert(url.to_string(), page.clone());
                return Ok(page);
            }
        };

        // Get any prior snippet for consistency
        let prior_snippet = {
            let snippets = self.snippet_cache.read().await;
            snippets.get(url).cloned()
        };

        let system = prompt::scrape_system(&self.world);
        let user = prompt::scrape_user(url, &site.content_description, prior_snippet.as_deref());

        let content = self.claude.chat_completion(&system, &user).await?;

        let page = SimPage {
            url: url.to_string(),
            content,
            raw_html: None,
        };

        // Cache and log
        {
            let mut cache = self.page_cache.write().await;
            cache.insert(url.to_string(), page.clone());
        }
        {
            let mut log = self.log.write().await;
            log.entries.push(LogEntry::Scrape {
                url: url.to_string(),
                page: page.clone(),
                timestamp: Utc::now(),
            });
        }

        info!(url, bytes = page.content.len(), "SimulatedWeb scrape");
        Ok(page)
    }

    /// Simulate fetching social media posts from a specific account.
    /// Returns empty vec if the profile doesn't match any in the world.
    pub async fn social_posts(
        &self,
        platform: &str,
        identifier: &str,
        limit: u32,
    ) -> Result<Vec<SimPost>> {
        let cache_key = format!("{platform}:{identifier}");

        // Check cache
        {
            let cache = self.social_cache.read().await;
            if let Some(posts) = cache.get(&cache_key) {
                return Ok(posts.iter().take(limit as usize).cloned().collect());
            }
        }

        // Find matching profile
        let profile = self.world.social_profiles.iter().find(|p| {
            p.platform.to_lowercase() == platform.to_lowercase() && p.identifier == identifier
        });

        let profile = match profile {
            Some(p) => p,
            None => {
                // No matching profile — return empty
                let mut cache = self.social_cache.write().await;
                cache.insert(cache_key, Vec::new());
                return Ok(Vec::new());
            }
        };

        let system = prompt::social_system(&self.world);
        let user = prompt::social_profile_user(platform, identifier, &profile.persona, limit);

        let response = self.claude.chat_completion(&system, &user).await?;
        let posts = parse_social_response(&response, platform);

        // Cache and log
        {
            let mut cache = self.social_cache.write().await;
            cache.insert(cache_key, posts.clone());
        }
        {
            let mut log = self.log.write().await;
            log.entries.push(LogEntry::Social {
                platform: platform.to_string(),
                identifier: identifier.to_string(),
                posts: posts.clone(),
                timestamp: Utc::now(),
            });
        }

        info!(
            platform,
            identifier,
            count = posts.len(),
            "SimulatedWeb social_posts"
        );
        Ok(posts)
    }

    /// Simulate searching for social posts by hashtags.
    /// Finds profiles whose persona matches topics, generates posts.
    pub async fn social_hashtags(&self, hashtags: &[String], limit: u32) -> Result<Vec<SimPost>> {
        let cache_key = format!("hashtags:{}", hashtags.join(","));

        // Check cache
        {
            let cache = self.social_cache.read().await;
            if let Some(posts) = cache.get(&cache_key) {
                return Ok(posts.iter().take(limit as usize).cloned().collect());
            }
        }

        // Find profiles whose persona mentions any of the hashtags/topics
        let matching_profiles: Vec<_> = self
            .world
            .social_profiles
            .iter()
            .filter(|p| {
                let persona_lower = p.persona.to_lowercase();
                hashtags
                    .iter()
                    .any(|h| persona_lower.contains(&h.to_lowercase()))
                    || self
                        .world
                        .topics
                        .iter()
                        .any(|t| persona_lower.contains(&t.to_lowercase()))
            })
            .collect();

        if matching_profiles.is_empty() {
            let mut cache = self.social_cache.write().await;
            cache.insert(cache_key, Vec::new());
            return Ok(Vec::new());
        }

        let system = prompt::social_system(&self.world);
        let user = prompt::social_hashtags_user(hashtags, limit);

        let response = self.claude.chat_completion(&system, &user).await?;
        let posts = parse_social_response(&response, "mixed");

        // Cache and log
        {
            let mut cache = self.social_cache.write().await;
            cache.insert(cache_key.clone(), posts.clone());
        }
        {
            let mut log = self.log.write().await;
            log.entries.push(LogEntry::Hashtags {
                hashtags: hashtags.to_vec(),
                posts: posts.clone(),
                timestamp: Utc::now(),
            });
        }

        info!(hashtags = ?hashtags, count = posts.len(), "SimulatedWeb social_hashtags");
        Ok(posts)
    }

    /// Access the world description.
    pub fn world(&self) -> &World {
        &self.world
    }

    /// Access the run log (all logged interactions so far).
    pub async fn run_log(&self) -> RunLog {
        self.log.read().await.clone()
    }
}

fn parse_search_response(response: &str) -> Vec<SimSearchResult> {
    let json_str = strip_code_fences(response);
    let parsed: SearchResponse = serde_json::from_str(json_str).unwrap_or_else(|e| {
        warn!(error = %e, "Failed to parse search response");
        SearchResponse {
            results: Vec::new(),
        }
    });
    parsed
        .results
        .into_iter()
        .map(|r| SimSearchResult {
            url: r.url,
            title: r.title,
            snippet: r.snippet,
        })
        .collect()
}

fn parse_social_response(response: &str, platform: &str) -> Vec<SimPost> {
    let json_str = strip_code_fences(response);
    let parsed: SocialResponse = serde_json::from_str(json_str).unwrap_or_else(|e| {
        warn!(error = %e, "Failed to parse social response");
        SocialResponse { posts: Vec::new() }
    });
    parsed
        .posts
        .into_iter()
        .map(|p| SimPost {
            content: p.content,
            author: p.author,
            url: p.url,
            platform: platform.to_string(),
        })
        .collect()
}

fn strip_code_fences(s: &str) -> &str {
    let trimmed = s.trim();
    trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|s| s.strip_suffix("```"))
        .unwrap_or(trimmed)
}
