//! Test harness for integration tests with real LLM calls and real Neo4j.
//!
//! Fakes the *data sources* (web pages, search results, social posts).
//! Uses real Claude, real Voyage embeddings, real Neo4j.

pub mod queries;

use rootsignal_common::CityNode;
use rootsignal_graph::testutil::neo4j_container;
use rootsignal_graph::{GraphClient, GraphWriter};
use rootsignal_scout::embedder::Embedder;
use rootsignal_scout::extractor::Extractor;
use rootsignal_scout::fixtures::{
    CorpusSearcher, FixtureScraper, FixtureSearcher, FixtureSocialScraper,
    LayeredSearcher, ScenarioSearcher, ScenarioSocialScraper,
};
use rootsignal_scout::scraper::{SearchResult, SocialPost, SocialScraper, WebSearcher};
use rootsignal_scout::scout::{Scout, ScoutStats};

/// Default test city node (Twin Cities).
fn default_city_node() -> CityNode {
    CityNode {
        id: uuid::Uuid::new_v4(),
        name: "Twin Cities (Minneapolis-St. Paul, Minnesota)".to_string(),
        slug: "twincities".to_string(),
        center_lat: 44.9778,
        center_lng: -93.2650,
        radius_km: 30.0,
        geo_terms: vec![
            "Minneapolis".to_string(), "St. Paul".to_string(), "Saint Paul".to_string(),
            "Twin Cities".to_string(), "Minnesota".to_string(), "Hennepin".to_string(),
            "Ramsey".to_string(), "MN".to_string(), "Mpls".to_string(),
        ],
        active: true,
        created_at: chrono::Utc::now(),
        last_scout_completed_at: None,
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
    social_override: Option<Box<dyn SocialScraper>>,
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
        self.searcher_override = Some(Box::new(
            ScenarioSearcher::new(&self.ctx.anthropic_key, scenario),
        ));
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
        self.social_override = Some(Box::new(scraper));
        self
    }

    /// Build the Scout and run a full cycle. Returns stats.
    pub async fn run(self) -> ScoutStats {
        let city_node = self.city_node;

        let searcher: Box<dyn WebSearcher> = match self.searcher_override {
            Some(s) => s,
            None => Box::new(FixtureSearcher::new(self.search_results)),
        };

        let social: Box<dyn SocialScraper> = match self.social_override {
            Some(s) => s,
            None => Box::new(FixtureSocialScraper::new(self.social_posts)),
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
            Box::new(FixtureScraper::new(&self.web_content)),
            searcher,
            social,
            &self.ctx.anthropic_key,
            city_node,
        );

        scout.run().await.expect("Scout run failed")
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
