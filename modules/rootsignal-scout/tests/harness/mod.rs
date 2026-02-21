//! Test harness for integration tests with real LLM calls and real Neo4j.
//!
//! Fakes the *data sources* (web pages, search results, social posts).
//! Uses real Claude, real Voyage embeddings, real Neo4j.

pub mod audit;
pub mod queries;
pub mod sim_adapter;

use std::sync::Arc;

use rootsignal_archive::FetchBackend;
use rootsignal_common::{CityNode, DiscoveryMethod, SourceNode, SourceRole};
use rootsignal_graph::testutil::neo4j_container;
use rootsignal_graph::{GraphClient, GraphWriter};
use rootsignal_scout::embedder::Embedder;
use rootsignal_scout::extractor::{self, Extractor};
use rootsignal_scout::fixtures::{
    CorpusSearcher, LayeredSearcher, MockArchive, ScenarioSearcher, ScenarioSocialScraper,
};
use rootsignal_scout::scout::{Scout, ScoutStats};
use rootsignal_scout::scraper::{SearchResult, SocialPost, WebSearcher, SocialScraper};
use rootsignal_scout::sources;
use simweb::{SimulatedWeb, World};

use sim_adapter::SimArchive;

/// Default test city node (Twin Cities).
fn default_city_node() -> CityNode {
    CityNode {
        name: "Twin Cities (Minneapolis-St. Paul, Minnesota)".to_string(),
        center_lat: 44.9778,
        center_lng: -93.2650,
        radius_km: 30.0,
        geo_terms: vec![
            "Minneapolis".to_string(),
            "St. Paul".to_string(),
            "Saint Paul".to_string(),
            "Twin Cities".to_string(),
            "Minnesota".to_string(),
            "Hennepin".to_string(),
            "Ramsey".to_string(),
            "MN".to_string(),
            "Mpls".to_string(),
        ],
    }
}

/// Owns a Neo4j container and API keys for the lifetime of a test.
/// The container handle is type-erased to avoid leaking testcontainers types.
pub struct TestContext {
    _container: Box<dyn std::any::Any + Send>,
    client: GraphClient,
    anthropic_key: String,
    voyage_key: String,
}

impl TestContext {
    /// Spin up Neo4j and validate API keys are present.
    /// Returns `None` if keys are missing (test should be skipped).
    pub async fn try_new() -> Option<Self> {
        dotenv_load();
        let anthropic_key = std::env::var("ANTHROPIC_API_KEY").ok()?;
        let voyage_key = std::env::var("VOYAGE_API_KEY").ok()?;

        let (container, client) = neo4j_container().await;

        rootsignal_graph::migrate::migrate(&client)
            .await
            .expect("Migration failed");

        Some(Self {
            _container: Box::new(container),
            client,
            anthropic_key,
            voyage_key,
        })
    }

    /// Direct access to the graph client for test assertions.
    pub fn client(&self) -> &GraphClient {
        &self.client
    }

    /// Create a GraphWriter for direct graph manipulation in tests.
    pub fn writer(&self) -> GraphWriter {
        GraphWriter::new(self.client.clone())
    }

    /// The Anthropic API key for this context.
    pub fn anthropic_key(&self) -> &str {
        &self.anthropic_key
    }

    /// Capture the current extractor prompt template (with `{city_name}` / `{today}` placeholders).
    pub fn baseline_extractor_prompt() -> String {
        extractor::build_system_prompt("{city_name}", 0.0, 0.0, &[])
    }

    /// Create a Scout wired to a SimulatedWeb, using a genome's extractor prompt.
    pub fn sim_scout_with_genome(
        &self,
        sim: Arc<SimulatedWeb>,
        city_node: CityNode,
        genome: &simweb::ScoutGenome,
    ) -> Scout {
        let prompt = genome.render_extractor_prompt(&city_node.name);
        let archive: Arc<dyn FetchBackend> = Arc::new(SimArchive::new(sim));
        Scout::new_for_test(
            self.client.clone(),
            Box::new(Extractor::with_system_prompt(&self.anthropic_key, prompt)),
            Box::new(Embedder::new(&self.voyage_key)),
            archive,
            &self.anthropic_key,
            city_node,
        )
    }

    /// Create a Scout wired to a SimulatedWeb for fuzzy integration tests.
    pub fn sim_scout(&self, sim: Arc<SimulatedWeb>, city_node: CityNode) -> Scout {
        let archive: Arc<dyn FetchBackend> = Arc::new(SimArchive::new(sim));
        Scout::new_for_test(
            self.client.clone(),
            Box::new(Extractor::new(
                &self.anthropic_key,
                &city_node.name,
                city_node.center_lat,
                city_node.center_lng,
            )),
            Box::new(Embedder::new(&self.voyage_key)),
            archive,
            &self.anthropic_key,
            city_node,
        )
    }

    /// Start building a scout run against this context's graph.
    pub fn scout(&self) -> ScoutBuilder<'_> {
        ScoutBuilder {
            ctx: self,
            city_node: default_city_node(),
            archive_override: None,
            pages: std::collections::HashMap::new(),
            search_results: Vec::new(),
            social_posts: Vec::new(),
            searcher_override: None,
            social_override: None,
        }
    }
}

/// Builder for configuring what data a scout run sees.
pub struct ScoutBuilder<'a> {
    ctx: &'a TestContext,
    city_node: CityNode,
    archive_override: Option<Arc<dyn FetchBackend>>,
    pages: std::collections::HashMap<String, String>,
    search_results: Vec<rootsignal_common::SearchResult>,
    social_posts: Vec<rootsignal_common::SocialPost>,
    searcher_override: Option<Box<dyn WebSearcher>>,
    social_override: Option<Arc<dyn SocialScraper>>,
}

impl<'a> ScoutBuilder<'a> {
    pub fn with_city(mut self, city_node: CityNode) -> Self {
        self.city_node = city_node;
        self
    }

    pub fn with_web_content(mut self, content: &str) -> Self {
        // Store as a default page for any URL
        self.pages.insert("*".to_string(), content.to_string());
        self
    }

    pub fn with_search_results(mut self, results: Vec<SearchResult>) -> Self {
        self.search_results = results.into_iter().map(|r| rootsignal_common::SearchResult {
            url: r.url,
            title: r.title,
            snippet: r.snippet,
        }).collect();
        self
    }

    pub fn with_social_posts(mut self, posts: Vec<SocialPost>) -> Self {
        self.social_posts = posts.into_iter().map(|p| rootsignal_common::SocialPost {
            content: p.content,
            author: p.author,
            url: p.url,
        }).collect();
        self
    }

    /// Override with a custom FetchBackend (e.g. SimArchive).
    pub fn with_archive(mut self, archive: Arc<dyn FetchBackend>) -> Self {
        self.archive_override = Some(archive);
        self
    }

    /// Use a ScenarioSearcher with default system prompt.
    pub fn with_scenario(mut self, scenario: &str) -> Self {
        let searcher = ScenarioSearcher::new(&self.ctx.anthropic_key, scenario);
        self.searcher_override = Some(Box::new(searcher));
        self
    }

    /// Use a pre-built ScenarioSearcher (with custom system prompt).
    pub fn with_search_scenario(mut self, searcher: ScenarioSearcher) -> Self {
        self.searcher_override = Some(Box::new(searcher));
        self
    }

    /// Use a LayeredSearcher: corpus-first, scenario-fallback.
    pub fn with_layered(mut self, corpus: CorpusSearcher, scenario: &str) -> Self {
        let fallback = ScenarioSearcher::new(&self.ctx.anthropic_key, scenario);
        self.searcher_override = Some(Box::new(LayeredSearcher::new(corpus, fallback)));
        self
    }

    /// Use a pre-built ScenarioSocialScraper.
    pub fn with_social_scenario(mut self, scraper: ScenarioSocialScraper) -> Self {
        self.social_override = Some(Arc::new(scraper));
        self
    }

    /// Build the Scout and run a full cycle. Returns stats.
    pub async fn run(self) -> ScoutStats {
        let city_node = self.city_node;

        let default_content = self.pages.get("*").cloned().unwrap_or_default();

        let archive: Arc<dyn FetchBackend> = match self.archive_override {
            Some(a) => a,
            None if self.searcher_override.is_some() || self.social_override.is_some() => {
                // Legacy mode: wrap old-style fixtures in FixtureArchive
                use rootsignal_scout::fixtures::{FixtureSearcher, FixtureSocialScraper};
                let searcher: Box<dyn WebSearcher> = match self.searcher_override {
                    Some(s) => s,
                    None => Box::new(FixtureSearcher::new(self.search_results.iter().map(|r| SearchResult {
                        url: r.url.clone(),
                        title: r.title.clone(),
                        snippet: r.snippet.clone(),
                    }).collect())),
                };
                let social: Arc<dyn SocialScraper> = match self.social_override {
                    Some(s) => s,
                    None => Arc::new(FixtureSocialScraper::new(self.social_posts.iter().map(|p| SocialPost {
                        content: p.content.clone(),
                        author: p.author.clone(),
                        url: p.url.clone(),
                    }).collect())),
                };
                Arc::new(FixtureArchive {
                    searcher,
                    page_content: default_content,
                    social,
                })
            }
            None => {
                // Build a MockArchive from the configured data
                let pages = if !default_content.is_empty() {
                    let mut map = self.pages.clone();
                    map.remove("*");
                    map
                } else {
                    self.pages
                };
                Arc::new(MockArchive::new(pages, self.search_results, self.social_posts))
            }
        };

        let scout = Scout::new_for_test(
            self.ctx.client.clone(),
            Box::new(Extractor::new(
                &self.ctx.anthropic_key,
                &city_node.name,
                city_node.center_lat,
                city_node.center_lng,
            )),
            Box::new(Embedder::new(&self.ctx.voyage_key)),
            archive,
            &self.ctx.anthropic_key,
            city_node,
        );

        scout.run().await.expect("Scout run failed")
    }
}

// --- FixtureArchive: wraps old-style test fixtures into FetchBackend ---

/// Adapts legacy fixture types (WebSearcher, PageScraper, SocialScraper) into
/// a single FetchBackend for backwards compatibility in integration tests.
struct FixtureArchive {
    searcher: Box<dyn WebSearcher>,
    page_content: String,
    social: Arc<dyn SocialScraper>,
}

#[async_trait::async_trait]
impl FetchBackend for FixtureArchive {
    async fn fetch_content(&self, target: &str) -> rootsignal_archive::Result<rootsignal_archive::FetchedContent> {
        let now = chrono::Utc::now();

        // Social targets
        if target.starts_with("social:") || target.contains("reddit.com/r/") || target.contains("instagram.com") || target.contains("x.com/") || target.contains("tiktok.com") || target.contains("bluesky.social") {
            // Build a dummy account for the social scraper
            let account = rootsignal_scout::scraper::SocialAccount {
                platform: rootsignal_scout::scraper::SocialPlatform::Reddit,
                identifier: target.to_string(),
            };
            let posts = self.social.search_posts(&account, 20).await
                .map_err(|e| rootsignal_archive::ArchiveError::FetchFailed(e.to_string()))?;
            let common_posts: Vec<rootsignal_common::SocialPost> = posts.into_iter().map(|p| rootsignal_common::SocialPost {
                content: p.content,
                author: p.author,
                url: p.url,
            }).collect();
            let text = common_posts.iter().map(|p| p.content.as_str()).collect::<Vec<_>>().join("\n");
            return Ok(rootsignal_archive::FetchedContent {
                target: target.to_string(),
                content: rootsignal_archive::Content::SocialPosts(common_posts),
                content_hash: format!("fixture-{}", target),
                fetched_at: now,
                duration_ms: 0,
                text,
            });
        }

        // Non-URL → search query
        if !target.starts_with("http") {
            let results = self.searcher.search(target, 10).await
                .map_err(|e| rootsignal_archive::ArchiveError::FetchFailed(e.to_string()))?;
            let common_results: Vec<rootsignal_common::SearchResult> = results.into_iter().map(|r| rootsignal_common::SearchResult {
                url: r.url,
                title: r.title,
                snippet: r.snippet,
            }).collect();
            let text = common_results.iter().map(|r| format!("{}: {}", r.title, r.snippet)).collect::<Vec<_>>().join("\n");
            return Ok(rootsignal_archive::FetchedContent {
                target: target.to_string(),
                content: rootsignal_archive::Content::SearchResults(common_results),
                content_hash: format!("fixture-{}", target),
                fetched_at: now,
                duration_ms: 0,
                text,
            });
        }

        // URL → return page content
        Ok(rootsignal_archive::FetchedContent {
            target: target.to_string(),
            content: rootsignal_archive::Content::Page(rootsignal_common::ScrapedPage {
                url: target.to_string(),
                markdown: self.page_content.clone(),
                raw_html: format!("<html><body>{}</body></html>", self.page_content),
                content_hash: format!("fixture-{}", target),
            }),
            content_hash: format!("fixture-{}", target),
            fetched_at: now,
            duration_ms: 0,
            text: self.page_content.clone(),
        })
    }

    async fn resolve_semantics(&self, _content: &rootsignal_archive::FetchedContent) -> rootsignal_archive::Result<rootsignal_common::ContentSemantics> {
        Err(rootsignal_archive::ArchiveError::Other(anyhow::anyhow!("FixtureArchive does not support semantics")))
    }
}

/// Convert a simweb World geography to a CityNode for Scout.
pub fn city_node_for(world: &World) -> CityNode {
    CityNode {
        name: format!(
            "{}, {}",
            world.geography.city, world.geography.state_or_region
        ),
        center_lat: world.geography.center_lat,
        center_lng: world.geography.center_lng,
        radius_km: 30.0,
        geo_terms: world.geography.local_terms.clone(),
    }
}

/// Helper to build a SearchResult with a URL that will be scraped.
pub fn search_result(url: &str, title: &str) -> SearchResult {
    SearchResult {
        url: url.to_string(),
        title: title.to_string(),
        snippet: String::new(),
    }
}

/// Seed Neo4j with SourceNodes derived from a World's sites and social profiles.
/// This ensures the scout has sources to schedule and scrape.
pub async fn seed_sources_from_world(writer: &GraphWriter, world: &World, city_slug: &str) {
    let now = chrono::Utc::now();

    for site in &world.sites {
        let cv = rootsignal_common::canonical_value(&site.url);
        let ck = sources::make_canonical_key(&site.url);

        let role = match site.kind.as_str() {
            "news" | "forum" | "reddit" => SourceRole::Tension,
            "nonprofit" | "government" | "service_directory" | "event_calendar" => {
                SourceRole::Response
            }
            _ => SourceRole::Mixed,
        };

        let source = SourceNode {
            id: uuid::Uuid::new_v4(),
            canonical_key: ck,
            canonical_value: cv,
            url: Some(site.url.clone()),
            discovery_method: DiscoveryMethod::Curated,
            created_at: now,
            last_scraped: None,
            last_produced_signal: None,
            signals_produced: 0,
            signals_corroborated: 0,
            consecutive_empty_runs: 0,
            active: true,
            gap_context: None,
            weight: 0.5,
            cadence_hours: None,
            avg_signals_per_scrape: 0.0,
            quality_penalty: 1.0,
            source_role: role,
            scrape_count: 0,
        };

        writer
            .upsert_source(&source)
            .await
            .expect("Failed to upsert site source");
    }

    for profile in &world.social_profiles {
        // Build URL from platform + identifier for proper canonical_value computation
        let url = match profile.platform.to_lowercase().as_str() {
            "reddit" => format!("https://www.reddit.com/r/{}/", profile.identifier),
            "instagram" => format!("https://www.instagram.com/{}/", profile.identifier),
            "facebook" => format!("https://www.facebook.com/{}", profile.identifier),
            "twitter" | "x" => format!("https://x.com/{}", profile.identifier),
            "tiktok" => format!("https://www.tiktok.com/@{}", profile.identifier),
            "bluesky" => format!("https://bsky.app/profile/{}", profile.identifier),
            _ => profile.identifier.clone(),
        };

        let cv = rootsignal_common::canonical_value(&url);
        let ck = sources::make_canonical_key(&url);

        let source = SourceNode {
            id: uuid::Uuid::new_v4(),
            canonical_key: ck,
            canonical_value: cv,
            url: Some(url),
            discovery_method: DiscoveryMethod::Curated,
            created_at: now,
            last_scraped: None,
            last_produced_signal: None,
            signals_produced: 0,
            signals_corroborated: 0,
            consecutive_empty_runs: 0,
            active: true,
            gap_context: None,
            weight: 0.5,
            cadence_hours: None,
            avg_signals_per_scrape: 0.0,
            quality_penalty: 1.0,
            source_role: SourceRole::Mixed,
            scrape_count: 0,
        };

        writer
            .upsert_source(&source)
            .await
            .expect("Failed to upsert social source");
    }
}

/// Load `.env` from the workspace root (two levels up from CARGO_MANIFEST_DIR).
/// Only sets vars that aren't already in the environment.
fn dotenv_load() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(".env");
    if let Ok(content) = std::fs::read_to_string(&path) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                if std::env::var(key.trim()).is_err() {
                    unsafe { std::env::set_var(key.trim(), value.trim()) };
                }
            }
        }
    }
}
