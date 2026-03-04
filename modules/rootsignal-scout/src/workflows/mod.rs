//! Scout pipeline configuration and engine builders.
//!
//! `ScoutDeps` holds shared deps. Engine builder methods construct seesaw
//! engines for each pipeline variant (scrape, full, news).

use std::sync::Arc;

use rootsignal_archive::{Archive, ArchiveConfig, PageBackend, SpawnDispatcher};
use rootsignal_graph::GraphClient;
use sqlx::PgPool;
use typed_builder::TypedBuilder;

use ai_client::Claude;
use crate::core::engine::{self, ScoutEngine, ScoutEngineDeps};
use crate::infra::embedder::TextEmbedder;
use crate::infra::util::HAIKU_MODEL;
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

    /// Construct engine deps with all per-invocation resources.
    ///
    /// Shared helper for both engine variants. `spent_cents` seeds the budget
    /// tracker so standalone workflows can carry forward prior spend.
    fn build_engine_deps(
        &self,
        scope: &rootsignal_common::ScoutScope,
        run_id: &str,
        spent_cents: u64,
        task_id: Option<&str>,
        completion_phase_status: Option<&str>,
    ) -> ScoutEngineDeps {
        let store: Arc<dyn SignalReader> = Arc::new(self.build_store());
        let embedder: Arc<dyn TextEmbedder> =
            Arc::new(crate::infra::embedder::Embedder::new(&self.voyage_api_key));
        let ai: Arc<dyn ai_client::Agent> = Arc::new(
            Claude::new(&self.anthropic_api_key, HAIKU_MODEL),
        );
        let extractor: Arc<dyn crate::core::extractor::SignalExtractor> =
            Arc::new(crate::core::extractor::Extractor::new(
                ai.clone(),
                scope.name.as_str(),
                scope.center_lat,
                scope.center_lng,
            ));
        let archive = create_archive(self);
        let budget = Arc::new(
            crate::domains::scheduling::activities::budget::BudgetTracker::new_with_spent(
                self.daily_budget_cents,
                spent_cents,
            ),
        );

        let mut deps = ScoutEngineDeps::new(store, embedder, run_id);
        deps.region = Some(scope.clone());
        deps.fetcher = Some(archive.clone() as Arc<dyn crate::traits::ContentFetcher>);
        deps.ai = Some(ai);
        deps.anthropic_api_key = Some(self.anthropic_api_key.clone());
        deps.graph_client = Some(self.graph_client.clone());
        deps.extractor = Some(extractor);
        deps.archive = Some(archive);
        deps.budget = Some(budget);
        deps.cancelled = Some(Arc::new(std::sync::atomic::AtomicBool::new(false)));
        deps.pg_pool = Some(self.pg_pool.clone());
        deps.task_id = task_id.map(String::from);
        deps.completion_phase_status = completion_phase_status.map(String::from);

        // Validate scrape-critical deps. build_engine_deps is the production
        // entry point for both scrape and full engines — catch configuration
        // errors here rather than panicking deep in the scrape phase.
        assert!(deps.extractor.is_some(), "scrape engine requires extractor — set deps.extractor before calling build_engine()");
        assert!(deps.fetcher.is_some(), "scrape engine requires fetcher — set deps.fetcher before calling build_engine()");

        deps
    }

    /// Build a scrape-chain engine: reap → schedule → scrape → enrichment →
    /// expansion → synthesis → finalize.
    ///
    /// Does NOT include situation_weaving or supervisor.
    pub fn build_scrape_engine(
        &self,
        scope: &rootsignal_common::ScoutScope,
        run_id: &str,
        task_id: Option<&str>,
        completion_phase_status: Option<&str>,
    ) -> ScoutEngine {
        let deps = self.build_engine_deps(scope, run_id, 0, task_id, completion_phase_status);
        engine::build_engine(deps)
    }

    /// Build a news-scan engine: NewsScanRequested → BeaconDetected.
    ///
    /// Minimal deps: no scope/region, no extractor.
    pub fn build_news_engine(&self, run_id: &str) -> ScoutEngine {
        let store: Arc<dyn SignalReader> = Arc::new(self.build_store());
        let embedder: Arc<dyn TextEmbedder> =
            Arc::new(crate::infra::embedder::Embedder::new(&self.voyage_api_key));
        let archive = create_archive(self);
        let budget = Arc::new(
            crate::domains::scheduling::activities::budget::BudgetTracker::new(
                self.daily_budget_cents,
            ),
        );

        let ai: Arc<dyn ai_client::Agent> = Arc::new(
            Claude::new(&self.anthropic_api_key, HAIKU_MODEL),
        );

        let mut deps = ScoutEngineDeps::new(store, embedder, run_id);
        deps.ai = Some(ai);
        deps.anthropic_api_key = Some(self.anthropic_api_key.clone());
        deps.graph_client = Some(self.graph_client.clone());
        deps.archive = Some(archive);
        deps.budget = Some(budget);
        deps.pg_pool = Some(self.pg_pool.clone());
        engine::build_news_engine(deps)
    }

    /// Build a full-chain engine: extends the scrape chain with
    /// situation_weaving → supervisor before finalize.
    ///
    /// `spent_cents` seeds the budget tracker so standalone workflows
    /// can carry forward prior spend from earlier phases.
    pub fn build_full_engine(
        &self,
        scope: &rootsignal_common::ScoutScope,
        run_id: &str,
        spent_cents: u64,
        task_id: Option<&str>,
        completion_phase_status: Option<&str>,
    ) -> ScoutEngine {
        let deps = self.build_engine_deps(scope, run_id, spent_cents, task_id, completion_phase_status);
        engine::build_full_engine(deps)
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
                std::env::var("OPENAI_API_KEY").unwrap_or_default(),
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

