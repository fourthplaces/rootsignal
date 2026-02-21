//! Test harness for integration tests with real LLM calls and real Neo4j.
//!
//! Fakes the *data sources* (web pages, search results, social posts).
//! Uses real Claude, real Voyage embeddings, real Neo4j.

pub mod audit;
pub mod queries;
pub mod sim_adapter;

use std::sync::Arc;

use rootsignal_common::{CityNode, DiscoveryMethod, SourceNode, SourceRole};
use rootsignal_graph::testutil::neo4j_container;
use rootsignal_graph::{GraphClient, GraphWriter};
use rootsignal_scout::embedder::Embedder;
use rootsignal_scout::extractor::{self, Extractor};
use rootsignal_scout::fixtures::{
    CorpusSearcher, FixtureScraper, FixtureSearcher, FixtureSocialScraper, LayeredSearcher,
    ScenarioSearcher, ScenarioSocialScraper,
};
use rootsignal_scout::scout::{Scout, ScoutStats};
use rootsignal_scout::scraper::{SearchResult, SocialPost, SocialScraper, WebSearcher};
use rootsignal_scout::sources;
use simweb::{SimulatedWeb, World};

use sim_adapter::{SimPageAdapter, SimSearchAdapter, SimSocialAdapter};

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
        Scout::with_deps(
            self.client.clone(),
            Box::new(Extractor::with_system_prompt(&self.anthropic_key, prompt)),
            Box::new(Embedder::new(&self.voyage_key)),
            Arc::new(SimPageAdapter::new(sim.clone())),
            Arc::new(SimSearchAdapter::new(sim.clone())),
            Arc::new(SimSocialAdapter::new(sim)),
            &self.anthropic_key,
            city_node,
        )
    }

    /// Create a Scout wired to a SimulatedWeb for fuzzy integration tests.
    pub fn sim_scout(&self, sim: Arc<SimulatedWeb>, city_node: CityNode) -> Scout {
        Scout::with_deps(
            self.client.clone(),
            Box::new(Extractor::new(
                &self.anthropic_key,
                &city_node.name,
                city_node.center_lat,
                city_node.center_lng,
            )),
            Box::new(Embedder::new(&self.voyage_key)),
            Arc::new(SimPageAdapter::new(sim.clone())),
            Arc::new(SimSearchAdapter::new(sim.clone())),
            Arc::new(SimSocialAdapter::new(sim)),
            &self.anthropic_key,
            city_node,
        )
    }

    /// Start building a scout run against this context's graph.
    pub fn scout(&self) -> ScoutBuilder<'_> {
        ScoutBuilder {
            ctx: self,
            city_node: default_city_node(),
            web_content: String::new(),
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
    web_content: String,
    search_results: Vec<SearchResult>,
    social_posts: Vec<SocialPost>,
    searcher_override: Option<Box<dyn WebSearcher>>,
    social_override: Option<Arc<dyn SocialScraper>>,
}

impl<'a> ScoutBuilder<'a> {
    pub fn with_city(mut self, city_node: CityNode) -> Self {
        self.city_node = city_node;
        self
    }

    pub fn with_web_content(mut self, content: &str) -> Self {
        self.web_content = content.to_string();
        self
    }

    pub fn with_search_results(mut self, results: Vec<SearchResult>) -> Self {
        self.search_results = results;
        self
    }

    pub fn with_social_posts(mut self, posts: Vec<SocialPost>) -> Self {
        self.social_posts = posts;
        self
    }

    // --- Searcher strategy overrides ---

    /// Use a pre-built CorpusSearcher.
    pub fn with_corpus(mut self, corpus: CorpusSearcher) -> Self {
        self.searcher_override = Some(Box::new(corpus));
        self
    }

    /// Use a ScenarioSearcher with default system prompt.
    pub fn with_scenario(mut self, scenario: &str) -> Self {
        self.searcher_override = Some(Box::new(ScenarioSearcher::new(
            &self.ctx.anthropic_key,
            scenario,
        )));
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

    // --- Social strategy overrides ---

    /// Use a pre-built ScenarioSocialScraper.
    pub fn with_social_scenario(mut self, scraper: ScenarioSocialScraper) -> Self {
        self.social_override = Some(Arc::new(scraper));
        self
    }

    /// Build the Scout and run a full cycle. Returns stats.
    pub async fn run(self) -> ScoutStats {
        let city_node = self.city_node;

        let searcher: Arc<dyn WebSearcher> = match self.searcher_override {
            Some(s) => Arc::from(s),
            None => Arc::new(FixtureSearcher::new(self.search_results)),
        };

        let social: Arc<dyn SocialScraper> = match self.social_override {
            Some(s) => s,
            None => Arc::new(FixtureSocialScraper::new(self.social_posts)),
        };

        let scout = Scout::with_deps(
            self.ctx.client.clone(),
            Box::new(Extractor::new(
                &self.ctx.anthropic_key,
                &city_node.name,
                city_node.center_lat,
                city_node.center_lng,
            )),
            Box::new(Embedder::new(&self.ctx.voyage_key)),
            Arc::new(FixtureScraper::new(&self.web_content)),
            searcher,
            social,
            &self.ctx.anthropic_key,
            city_node,
        );

        scout.run().await.expect("Scout run failed")
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
