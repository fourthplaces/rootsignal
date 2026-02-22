//! Test harness for integration tests with real LLM calls and real Neo4j.
//!
//! Fakes the *data sources* (web pages, search results, social posts).
//! Uses real Claude, real Voyage embeddings, real Neo4j.

pub mod archive_seed;
pub mod audit;
pub mod queries;
pub mod sim_adapter;

use std::sync::Arc;

use sqlx::PgPool;
use uuid::Uuid;

use rootsignal_archive::{FetchBackend, Replay, Seeder};
use rootsignal_common::{ScoutScope, DiscoveryMethod, SearchResult, SocialPost, SourceNode, SourceRole};
use rootsignal_graph::testutil::neo4j_container;
use rootsignal_graph::{GraphClient, GraphWriter};
use rootsignal_scout::embedder::Embedder;
use rootsignal_scout::extractor::{self, Extractor};
use rootsignal_scout::fixtures::{
    CorpusSearcher, FixtureArchive, FixtureSocialKind, FixtureSocialScraper,
    LayeredSearcher, MockArchive, ScenarioSearcher, ScenarioSocialScraper,
};
use rootsignal_scout::scout::{Scout, ScoutStats};
use rootsignal_scout::sources;
use simweb::{SimulatedWeb, World};

use sim_adapter::SimArchive;

/// Default test scope (Twin Cities).
fn default_scope() -> ScoutScope {
    ScoutScope {
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
    pg_pool: Option<PgPool>,
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
            pg_pool: None,
        })
    }

    /// Spin up Neo4j + Postgres. Returns `None` if keys or DATABASE_URL are missing.
    pub async fn try_new_with_pg() -> Option<Self> {
        dotenv_load();
        let anthropic_key = std::env::var("ANTHROPIC_API_KEY").ok()?;
        let voyage_key = std::env::var("VOYAGE_API_KEY").ok()?;
        let database_url = std::env::var("DATABASE_URL").ok()?;

        let (container, client) = neo4j_container().await;

        rootsignal_graph::migrate::migrate(&client)
            .await
            .expect("Migration failed");

        let pg_pool = PgPool::connect(&database_url)
            .await
            .expect("Failed to connect to Postgres");

        Some(Self {
            _container: Box::new(container),
            client,
            anthropic_key,
            voyage_key,
            pg_pool: Some(pg_pool),
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

    /// Create a Seeder for writing archived content into Postgres.
    /// Panics if no Postgres pool is available (use `try_new_with_pg`).
    pub async fn seeder(&self, region_slug: &str) -> (Seeder, Uuid) {
        let pool = self.pg_pool.clone().expect("No Postgres pool — use try_new_with_pg()");
        let run_id = Uuid::new_v4();
        let seeder = Seeder::new(pool, run_id, region_slug)
            .await
            .expect("Failed to create Seeder");
        (seeder, run_id)
    }

    /// Create a Replay backend for reading archived content from Postgres.
    /// Panics if no Postgres pool is available (use `try_new_with_pg`).
    pub fn replay(&self, run_id: Uuid) -> Arc<dyn FetchBackend> {
        let pool = self.pg_pool.clone().expect("No Postgres pool — use try_new_with_pg()");
        Arc::new(Replay::for_run(pool, run_id))
    }

    /// Capture the current extractor prompt template (with `{region_name}` / `{today}` placeholders).
    pub fn baseline_extractor_prompt() -> String {
        extractor::build_system_prompt("{region_name}", 0.0, 0.0, &[])
    }

    /// Create a Scout wired to a SimulatedWeb, using a genome's extractor prompt.
    pub fn sim_scout_with_genome(
        &self,
        sim: Arc<SimulatedWeb>,
        scope: ScoutScope,
        genome: &simweb::ScoutGenome,
    ) -> Scout {
        let prompt = genome.render_extractor_prompt(&scope.name);
        let archive: Arc<dyn FetchBackend> = Arc::new(SimArchive::new(sim));
        Scout::new_for_test(
            self.client.clone(),
            Box::new(Extractor::with_system_prompt(&self.anthropic_key, prompt)),
            Arc::new(Embedder::new(&self.voyage_key)),
            archive,
            &self.anthropic_key,
            scope,
        )
    }

    /// Create a Scout wired to a SimulatedWeb for fuzzy integration tests.
    pub fn sim_scout(&self, sim: Arc<SimulatedWeb>, scope: ScoutScope) -> Scout {
        let archive: Arc<dyn FetchBackend> = Arc::new(SimArchive::new(sim));
        Scout::new_for_test(
            self.client.clone(),
            Box::new(Extractor::new(
                &self.anthropic_key,
                &scope.name,
                scope.center_lat,
                scope.center_lng,
            )),
            Arc::new(Embedder::new(&self.voyage_key)),
            archive,
            &self.anthropic_key,
            scope,
        )
    }

    /// Start building a scout run against this context's graph.
    pub fn scout(&self) -> ScoutBuilder<'_> {
        ScoutBuilder {
            ctx: self,
            scope: default_scope(),
            archive_override: None,
            pages: std::collections::HashMap::new(),
            search_results: Vec::new(),
            social_posts: Vec::new(),
            searcher_kind: None,
            social_kind: None,
        }
    }
}

/// Builder for configuring what data a scout run sees.
pub struct ScoutBuilder<'a> {
    ctx: &'a TestContext,
    scope: ScoutScope,
    archive_override: Option<Arc<dyn FetchBackend>>,
    pages: std::collections::HashMap<String, String>,
    search_results: Vec<SearchResult>,
    social_posts: Vec<SocialPost>,
    searcher_kind: Option<SearcherKind>,
    social_kind: Option<FixtureSocialKind>,
}

enum SearcherKind {
    Scenario(ScenarioSearcher),
    Layered(LayeredSearcher),
}

impl<'a> ScoutBuilder<'a> {
    pub fn with_city(mut self, scope: ScoutScope) -> Self {
        self.scope = scope;
        self
    }

    pub fn with_web_content(mut self, content: &str) -> Self {
        self.pages.insert("*".to_string(), content.to_string());
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

    /// Override with a custom FetchBackend (e.g. SimArchive).
    pub fn with_archive(mut self, archive: Arc<dyn FetchBackend>) -> Self {
        self.archive_override = Some(archive);
        self
    }

    /// Use a ScenarioSearcher with default system prompt.
    pub fn with_scenario(mut self, scenario: &str) -> Self {
        let searcher = ScenarioSearcher::new(&self.ctx.anthropic_key, scenario);
        self.searcher_kind = Some(SearcherKind::Scenario(searcher));
        self
    }

    /// Use a pre-built ScenarioSearcher (with custom system prompt).
    pub fn with_search_scenario(mut self, searcher: ScenarioSearcher) -> Self {
        self.searcher_kind = Some(SearcherKind::Scenario(searcher));
        self
    }

    /// Use a LayeredSearcher: corpus-first, scenario-fallback.
    pub fn with_layered(mut self, corpus: CorpusSearcher, scenario: &str) -> Self {
        let fallback = ScenarioSearcher::new(&self.ctx.anthropic_key, scenario);
        self.searcher_kind = Some(SearcherKind::Layered(LayeredSearcher::new(corpus, fallback)));
        self
    }

    /// Use a pre-built ScenarioSocialScraper.
    pub fn with_social_scenario(mut self, scraper: ScenarioSocialScraper) -> Self {
        self.social_kind = Some(FixtureSocialKind::Scenario(scraper));
        self
    }

    /// Build the Scout and run a full cycle. Returns stats.
    pub async fn run(self) -> ScoutStats {
        let scope = self.scope;

        let default_content = self.pages.get("*").cloned().unwrap_or_default();

        let archive: Arc<dyn FetchBackend> = match self.archive_override {
            Some(a) => a,
            None if self.searcher_kind.is_some() || self.social_kind.is_some() => {
                let social = self.social_kind.unwrap_or_else(|| {
                    FixtureSocialKind::Static(FixtureSocialScraper::new(self.social_posts))
                });

                match self.searcher_kind {
                    Some(SearcherKind::Scenario(s)) => {
                        Arc::new(FixtureArchive::with_scenario_searcher(s, default_content, social))
                    }
                    Some(SearcherKind::Layered(l)) => {
                        Arc::new(FixtureArchive::with_layered_searcher(l, default_content, social))
                    }
                    None => {
                        Arc::new(FixtureArchive::with_static_searcher(self.search_results, default_content, social))
                    }
                }
            }
            None => {
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
                &scope.name,
                scope.center_lat,
                scope.center_lng,
            )),
            Arc::new(Embedder::new(&self.ctx.voyage_key)),
            archive,
            &self.ctx.anthropic_key,
            scope,
        );

        scout.run().await.expect("Scout run failed")
    }
}

/// Convert a simweb World geography to a ScoutScope.
pub fn scope_for(world: &World) -> ScoutScope {
    ScoutScope {
        name: format!(
            "{}, {}",
            world.geography.name, world.geography.state_or_region
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
