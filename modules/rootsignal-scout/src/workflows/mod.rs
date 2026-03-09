//! Scout pipeline configuration and engine builders.
//!
//! `ScoutDeps` holds shared deps. Engine builder methods construct seesaw
//! engines for each pipeline variant (scrape, full, news).

use std::sync::Arc;

use rootsignal_archive::{Archive, ArchiveConfig, PageBackend, SpawnDispatcher};
use rootsignal_graph::{GraphClient, GraphReader, GraphQueries};
use sqlx::PgPool;
use typed_builder::TypedBuilder;
use uuid::Uuid;

use ai_client::{Claude, FallbackAgent, OpenAi};
use crate::core::engine::{self, ScoutEngine, ScoutEngineDeps};
use crate::core::postgres_store::PostgresStore;
use crate::infra::embedder::TextEmbedder;
use crate::traits::{ContentFetcher, SignalReader};

/// Shared dependency container for all scout workflows.
///
/// Mirrors mntogether's `ServerDeps` pattern. Holds long-lived, cloneable
/// resources. Per-invocation resources (Archive, Embedder, Extractor) are
/// constructed from these deps at the start of each workflow invocation.
#[derive(Clone, TypedBuilder)]
pub struct ScoutDeps {
    pub graph_client: GraphClient,
    pub pg_pool: PgPool,
    pub anthropic_api_key: String,
    pub gemini_api_key: String,
    pub openai_api_key: String,
    pub voyage_api_key: String,
    pub serper_api_key: String,
    #[builder(default)]
    pub apify_api_key: String,
    pub daily_budget_cents: u64,
    #[builder(default)]
    pub browserless_url: Option<String>,
    #[builder(default)]
    pub browserless_token: Option<String>,
    #[builder(default = 50)]
    pub max_web_queries_per_run: usize,
}

impl ScoutDeps {
    /// Build the production SignalReader from these deps.
    pub fn build_store(&self) -> crate::store::event_sourced::EventSourcedReader {
        crate::store::build_signal_reader(self.graph_client.clone())
    }

    // -----------------------------------------------------------------
    // Base dep construction — single source of truth for per-run wiring
    // -----------------------------------------------------------------

    /// Wire all per-invocation infrastructure deps.
    ///
    /// Returns `(deps, ai)` — callers add the scope-specific parts
    /// (extractor) on top.
    /// `budget_cents` is the effective budget for this run (already computed by caller).
    fn build_base_deps(
        &self,
        run_id: Uuid,
        spent_cents: u64,
        budget_cents: Option<u64>,
    ) -> (ScoutEngineDeps, Arc<dyn ai_client::Agent>) {
        let store: Arc<dyn SignalReader> = Arc::new(self.build_store());
        let embedder: Arc<dyn TextEmbedder> =
            Arc::new(crate::infra::embedder::Embedder::new(&self.voyage_api_key));
        let ai: Arc<dyn ai_client::Agent> = Arc::new(FallbackAgent::new(vec![
            Arc::new(OpenAi::new(&self.openai_api_key, ai_client::models::GPT_5_MINI)),
            Arc::new(Claude::new(&self.anthropic_api_key, ai_client::models::SONNET_4_6)),
        ]));
        let archive = create_archive(self);
        let limit = budget_cents.unwrap_or(self.daily_budget_cents);
        let budget = Arc::new(
            crate::domains::scheduling::activities::budget::BudgetTracker::new_with_spent(
                limit,
                spent_cents,
            ),
        );

        let mut deps = ScoutEngineDeps::new(store, embedder, run_id);
        deps.fetcher = Some(archive.clone() as Arc<dyn ContentFetcher>);
        deps.ai = Some(ai.clone());
        deps.anthropic_api_key = Some(self.anthropic_api_key.clone());
        deps.graph = Some(Arc::new(GraphReader::new(self.graph_client.clone())) as Arc<dyn GraphQueries>);
        deps.graph_client = Some(self.graph_client.clone());
        deps.archive = Some(archive);
        deps.budget = Some(budget);
        deps.pg_pool = Some(self.pg_pool.clone());

        (deps, ai)
    }

    /// Build an extractor scoped to a region (or neutral for unscoped runs).
    fn make_extractor(
        ai: &Arc<dyn ai_client::Agent>,
        scope: Option<&rootsignal_common::ScoutScope>,
    ) -> Arc<dyn crate::core::extractor::SignalExtractor> {
        let (name, lat, lng) = match scope {
            Some(s) => (s.name.as_str(), s.center_lat, s.center_lng),
            None => ("Unscoped", 0.0, 0.0),
        };
        Arc::new(crate::core::extractor::Extractor::new(ai.clone(), name, lat, lng))
    }

    // -----------------------------------------------------------------
    // Region-scoped builders (bootstrap, scrape, weave, full)
    // -----------------------------------------------------------------

    /// Construct engine deps for a region-scoped run.
    fn build_region_deps(
        &self,
        scope: &rootsignal_common::ScoutScope,
        run_id: Uuid,
        spent_cents: u64,
        budget_cents: Option<u64>,
    ) -> ScoutEngineDeps {
        let (mut deps, ai) = self.build_base_deps(run_id, spent_cents, budget_cents);
        deps.extractor = Some(Self::make_extractor(&ai, Some(scope)));
        deps
    }

    /// Build a scrape-chain engine: reap → schedule → scrape → enrichment →
    /// expansion → synthesis → finalize.
    ///
    /// Does NOT include situation_weaving or supervisor.
    pub fn build_scrape_engine(
        &self,
        scope: &rootsignal_common::ScoutScope,
        run_id: Uuid,
        budget_cents: Option<u64>,
    ) -> ScoutEngine {
        let deps = self.build_region_deps(scope, run_id, 0, budget_cents);
        engine::build_engine(deps, self.make_store(run_id))
    }

    /// Build a full-chain engine: extends the scrape chain with
    /// situation_weaving → supervisor before finalize.
    ///
    /// `spent_cents` seeds the budget tracker so standalone workflows
    /// can carry forward prior spend from earlier phases.
    pub fn build_full_engine(
        &self,
        scope: &rootsignal_common::ScoutScope,
        run_id: Uuid,
        spent_cents: u64,
        budget_cents: Option<u64>,
    ) -> ScoutEngine {
        let deps = self.build_region_deps(scope, run_id, spent_cents, budget_cents);
        engine::build_full_engine(deps, self.make_store(run_id))
    }

    /// Build a weave-only engine: cross-signal synthesis at any region level.
    ///
    /// Includes synthesis, situation_weaving, supervisor — excludes scrape/discovery/enrichment/expansion.
    pub fn build_weave_engine(
        &self,
        scope: &rootsignal_common::ScoutScope,
        run_id: Uuid,
        budget_cents: Option<u64>,
    ) -> ScoutEngine {
        let deps = self.build_region_deps(scope, run_id, 0, budget_cents);
        engine::build_weave_engine(deps, self.make_store(run_id))
    }

    /// Build engine deps for resuming a crashed run.
    pub fn build_engine_deps_for_resume(
        &self,
        scope: &rootsignal_common::ScoutScope,
        run_id: Uuid,
    ) -> ScoutEngineDeps {
        self.build_region_deps(scope, run_id, 0, None)
    }

    // -----------------------------------------------------------------
    // Source-targeted builder
    // -----------------------------------------------------------------

    /// Build a source-targeted engine: scrape specific input sources.
    ///
    /// When `region` is provided (via WATCHES edge), enrichment/expansion/synthesis
    /// get geographic context. When None, those phases skip gracefully.
    pub fn build_source_engine(
        &self,
        region: Option<&rootsignal_common::RegionNode>,
        run_id: Uuid,
        budget_cents: Option<u64>,
    ) -> ScoutEngine {
        let scope = region.map(rootsignal_common::ScoutScope::from);
        let (mut deps, ai) = self.build_base_deps(run_id, 0, budget_cents);
        deps.extractor = Some(Self::make_extractor(&ai, scope.as_ref()));
        engine::build_engine(deps, self.make_store(run_id))
    }

    // -----------------------------------------------------------------
    // News engine (no region, no extractor)
    // -----------------------------------------------------------------

    /// Build a news-scan engine: NewsScanRequested → scan RSS → extract signals.
    pub fn build_news_engine(&self, run_id: Uuid, budget_cents: Option<u64>) -> ScoutEngine {
        let (deps, _ai) = self.build_base_deps(run_id, 0, budget_cents);
        engine::build_news_engine(deps, self.make_store(run_id))
    }

    // -----------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------

    /// Create a PostgresStore scoped to a run_id (used as correlation_id).
    fn make_store(&self, run_id: Uuid) -> Option<Arc<PostgresStore>> {
        Some(Arc::new(PostgresStore::new(self.pg_pool.clone(), run_id)))
    }

    /// Convenience constructor from Config — keeps API-side construction clean.
    pub fn from_config(
        graph_client: GraphClient,
        pg_pool: PgPool,
        config: &rootsignal_common::Config,
    ) -> Self {
        Self::builder()
            .graph_client(graph_client)
            .pg_pool(pg_pool)
            .anthropic_api_key(config.anthropic_api_key.clone())
            .gemini_api_key(config.gemini_api_key.clone())
            .openai_api_key(config.openai_api_key.clone())
            .voyage_api_key(config.voyage_api_key.clone())
            .serper_api_key(config.serper_api_key.clone())
            .apify_api_key(config.apify_api_key.clone())
            .daily_budget_cents(config.daily_budget_cents)
            .browserless_url(config.browserless_url.clone())
            .browserless_token(config.browserless_token.clone())
            .max_web_queries_per_run(config.max_web_queries_per_run)
            .build()
    }
}

/// Create an `Archive` from the shared deps.
///
/// Each workflow invocation should call this to get a fresh archive instance.
pub fn create_archive(deps: &ScoutDeps) -> Arc<Archive> {
    let archive_config = ArchiveConfig {
        page_backend: match deps.browserless_url {
            Some(ref url) => PageBackend::Browserless {
                base_url: url.clone(),
                token: deps.browserless_token.clone(),
            },
            None => PageBackend::Chrome,
        },
        serper_api_key: deps.serper_api_key.clone(),
        apify_api_key: if deps.apify_api_key.is_empty() {
            None
        } else {
            Some(deps.apify_api_key.clone())
        },
    };

    let dispatcher: Option<Arc<dyn rootsignal_archive::WorkflowDispatcher>> =
        if !deps.anthropic_api_key.is_empty() {
            Some(Arc::new(SpawnDispatcher::new(
                deps.pg_pool.clone(),
                deps.anthropic_api_key.clone(),
                deps.openai_api_key.clone(),
            )))
        } else {
            None
        };

    Arc::new(Archive::new(
        deps.pg_pool.clone(),
        archive_config,
        dispatcher,
    ))
}

