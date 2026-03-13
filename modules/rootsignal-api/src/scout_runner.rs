//! Spawns causal engines in tokio tasks. Each GraphQL mutation creates a
//! short-lived task that builds the appropriate engine variant, emits the
//! entry event, and settles.
//!
//! On startup, `resume_incomplete_runs` queries for runs that crashed mid-flight,
//! reclaims their stale queue entries, and calls `settle()` to finish them.
//!
//! Cancellation is DB-backed via `seesaw_cancellations`. Any server can cancel
//! any run — no engine handle or in-process flag needed.

use std::sync::Arc;

use rootsignal_common::ScoutScope;
use tracing::{info, warn};

use rootsignal_scout::core::engine;
use rootsignal_scout::core::postgres_store::PostgresStore;
use rootsignal_scout::core::run_scope::RunScope;
use causal::reactor_queue::ReactorQueue;
use rootsignal_scout::domains::lifecycle::events::LifecycleEvent;
use rootsignal_scout::workflows::ScoutDeps;

/// Options for automatic chain orchestration (scout → coalesce → weave).
#[derive(Clone, Default)]
pub struct ChainOpts {
    pub parent_run_id: Option<String>,
    pub schedule_id: Option<String>,
    pub chain: bool,
}

/// Holds `Arc<ScoutDeps>`. Spawns causal engines in tokio tasks.
/// Cancellation goes through Postgres (seesaw_cancellations table).
#[derive(Clone)]
pub struct ScoutRunner {
    deps: Arc<ScoutDeps>,
}

impl ScoutRunner {
    pub fn new(deps: Arc<ScoutDeps>) -> Self {
        Self { deps }
    }

    /// Spawn a news scan (global, no region).
    pub async fn run_news_scan(&self) {
        let deps = self.deps.clone();
        let run_id = uuid::Uuid::new_v4();

        info!("Spawning news scan");

        tokio::spawn(async move {
            let engine = deps.build_news_engine(run_id);

            if let Err(e) = engine
                .emit(LifecycleEvent::NewsScanRequested)
                .correlation_id(run_id)
                .settled()
                .await
            {
                warn!(error = %e, "News scan failed");
            } else {
                info!("News scan completed");
            }
        });
    }

    // --- Flow-based methods (Region + Source) ---

    /// Spawn a bootstrap flow for a region: discover sources.
    pub async fn run_bootstrap(&self, region_id: &str, scope: &ScoutScope) {
        let deps = self.deps.clone();
        let region_id = region_id.to_string();
        let scope = scope.clone();
        let run_id = uuid::Uuid::new_v4();
        let budget = crate::db::models::budget::effective_budget(
            &deps.pg_pool, deps.daily_budget_cents,
        ).await;

        info!(region_id = region_id.as_str(), %run_id, "Spawning bootstrap flow");

        tokio::spawn(async move {
            let run_scope = RunScope::Region(scope.clone());
            let engine = deps.build_scrape_engine(&scope, run_id);
            let result = engine
                .emit(LifecycleEvent::ScoutRunRequested {
                    run_id,
                    scope: run_scope,
                    budget_cents: budget,
                    region_id: Some(region_id.clone()),
                    flow_type: "bootstrap".into(),
                    source_ids: None,
                    task_id: std::env::var("FLY_MACHINE_ID").ok(),
                    parent_run_id: None,
                    schedule_id: None,
                    run_at: None,
                })
                .correlation_id(run_id)
                .settled()
                .await;

            if let Err(e) = result {
                warn!(region_id = region_id.as_str(), error = %e, "Bootstrap flow failed");
            } else {
                info!(region_id = region_id.as_str(), "Bootstrap flow completed");
            }
        });
    }

    /// Spawn a scrape flow for a region: auto-bootstraps if no sources, then scrapes + extracts.
    pub async fn run_scrape(&self, region_id: &str, scope: &ScoutScope) {
        self.run_scrape_with_chain(region_id, scope, ChainOpts::default()).await;
    }

    /// Spawn a scrape flow with optional chain orchestration.
    pub async fn run_scrape_with_chain(&self, region_id: &str, scope: &ScoutScope, chain: ChainOpts) {
        let runner = self.clone();
        let deps = self.deps.clone();
        let region_id = region_id.to_string();
        let scope = scope.clone();
        let run_id = uuid::Uuid::new_v4();
        let budget = crate::db::models::budget::effective_budget(
            &deps.pg_pool, deps.daily_budget_cents,
        ).await;

        info!(region_id = region_id.as_str(), %run_id, chain = chain.chain, "Spawning scrape flow");

        tokio::spawn(async move {
            let run_scope = RunScope::Region(scope.clone());
            let engine = deps.build_scrape_engine(&scope, run_id);
            let result = engine
                .emit(LifecycleEvent::ScoutRunRequested {
                    run_id,
                    scope: run_scope,
                    budget_cents: budget,
                    region_id: Some(region_id.clone()),
                    flow_type: "scrape".into(),
                    source_ids: None,
                    task_id: std::env::var("FLY_MACHINE_ID").ok(),
                    parent_run_id: chain.parent_run_id,
                    schedule_id: chain.schedule_id,
                    run_at: None,
                })
                .correlation_id(run_id)
                .settled()
                .await;

            if let Err(e) = result {
                warn!(region_id = region_id.as_str(), error = %e, "Scrape flow failed");
                return;
            }

            info!(region_id = region_id.as_str(), "Scrape flow completed");

            // Chain: scout → coalesce (if enabled and succeeded)
            if chain.chain {
                runner.maybe_chain_coalesce(&region_id, &scope, &run_id.to_string()).await;
            }
        });
    }

    /// Spawn a weave flow for a region: situation weaving as independent workflow.
    pub async fn run_weave(&self, region_id: &str, scope: &ScoutScope) {
        self.run_weave_with_chain(region_id, scope, ChainOpts::default()).await;
    }

    /// Spawn a weave flow with optional chain orchestration.
    pub async fn run_weave_with_chain(&self, region_id: &str, scope: &ScoutScope, chain: ChainOpts) {
        let deps = self.deps.clone();
        let region_id = region_id.to_string();
        let scope = scope.clone();
        let run_id = uuid::Uuid::new_v4();
        let budget = crate::db::models::budget::effective_budget(
            &deps.pg_pool, deps.daily_budget_cents,
        ).await;

        info!(region_id = region_id.as_str(), %run_id, "Spawning weave flow");

        tokio::spawn(async move {
            let engine = deps.build_weave_engine(&scope, run_id);
            let result = engine
                .emit(LifecycleEvent::GenerateSituationsRequested {
                    run_id,
                    region: scope.clone(),
                    budget_cents: budget,
                    region_id: Some(region_id.clone()),
                    task_id: std::env::var("FLY_MACHINE_ID").ok(),
                    parent_run_id: chain.parent_run_id,
                    schedule_id: chain.schedule_id,
                    run_at: None,
                })
                .correlation_id(run_id)
                .settled()
                .await;

            if let Err(e) = result {
                warn!(region_id = region_id.as_str(), error = %e, "Weave flow failed");
            } else {
                info!(region_id = region_id.as_str(), "Weave flow completed");
            }
        });
    }

    /// Spawn a coalesce flow: seed from a specific signal, coalescing only (no weaving).
    pub async fn run_coalesce_signal(
        &self,
        region_id: &str,
        scope: &ScoutScope,
        signal_id: uuid::Uuid,
    ) {
        let deps = self.deps.clone();
        let region_id = region_id.to_string();
        let scope = scope.clone();
        let run_id = uuid::Uuid::new_v4();
        let budget = crate::db::models::budget::effective_budget(
            &deps.pg_pool, deps.daily_budget_cents,
        ).await;

        info!(region_id = region_id.as_str(), %signal_id, %run_id, "Spawning coalesce flow");

        tokio::spawn(async move {
            let engine = deps.build_coalesce_engine(&scope, run_id);
            let result = engine
                .emit(LifecycleEvent::CoalesceRequested {
                    run_id,
                    region: scope.clone(),
                    seed_signal_id: Some(signal_id),
                    budget_cents: budget,
                    region_id: Some(region_id.clone()),
                    task_id: std::env::var("FLY_MACHINE_ID").ok(),
                    parent_run_id: None,
                    schedule_id: None,
                    run_at: None,
                })
                .correlation_id(run_id)
                .settled()
                .await;

            if let Err(e) = result {
                warn!(region_id = region_id.as_str(), error = %e, "Coalesce flow failed");
            } else {
                info!(region_id = region_id.as_str(), "Coalesce flow completed");
            }
        });
    }

    /// Spawn a region-scoped coalesce flow (no seed signal) — used by chain orchestration.
    pub async fn run_coalesce_for_region(&self, region_id: &str, scope: &ScoutScope, chain: ChainOpts) {
        let runner = self.clone();
        let deps = self.deps.clone();
        let region_id = region_id.to_string();
        let scope = scope.clone();
        let run_id = uuid::Uuid::new_v4();
        let budget = crate::db::models::budget::effective_budget(
            &deps.pg_pool, deps.daily_budget_cents,
        ).await;

        info!(region_id = region_id.as_str(), %run_id, chain = chain.chain, "Spawning region coalesce flow");

        tokio::spawn(async move {
            let engine = deps.build_coalesce_engine(&scope, run_id);
            let result = engine
                .emit(LifecycleEvent::CoalesceRequested {
                    run_id,
                    region: scope.clone(),
                    seed_signal_id: None,
                    budget_cents: budget,
                    region_id: Some(region_id.clone()),
                    task_id: std::env::var("FLY_MACHINE_ID").ok(),
                    parent_run_id: chain.parent_run_id,
                    schedule_id: chain.schedule_id,
                    run_at: None,
                })
                .correlation_id(run_id)
                .settled()
                .await;

            if let Err(e) = result {
                warn!(region_id = region_id.as_str(), error = %e, "Region coalesce flow failed");
                return;
            }

            info!(region_id = region_id.as_str(), "Region coalesce flow completed");

            // Chain: coalesce → weave (if enabled and succeeded)
            if chain.chain {
                runner.maybe_chain_weave(&region_id, &scope, &run_id.to_string()).await;
            }
        });
    }

    // --- Chain orchestration helpers ---
    // Projection writes complete within the dispatch cycle before settle() returns.
    // If seesaw moves to async projections, this assumption breaks.

    async fn maybe_chain_coalesce(&self, region_id: &str, scope: &ScoutScope, parent_run_id: &str) {
        let pool = &self.deps.pg_pool;

        let succeeded = crate::db::scout_run::run_succeeded(pool, parent_run_id)
            .await
            .unwrap_or(false);
        if !succeeded {
            info!(parent_run_id, "Scrape run did not succeed — skipping coalesce chain");
            return;
        }

        let already_has_child = crate::db::scout_run::has_child_run(pool, parent_run_id, "coalesce")
            .await
            .unwrap_or(false);
        if already_has_child {
            info!(parent_run_id, "Coalesce child already exists — skipping");
            return;
        }

        info!(parent_run_id, "Chaining: scout → coalesce");
        self.run_coalesce_for_region(
            region_id,
            scope,
            ChainOpts {
                parent_run_id: Some(parent_run_id.to_string()),
                schedule_id: None,
                chain: true,
            },
        ).await;
    }

    async fn maybe_chain_weave(&self, region_id: &str, scope: &ScoutScope, parent_run_id: &str) {
        let pool = &self.deps.pg_pool;

        let succeeded = crate::db::scout_run::run_succeeded(pool, parent_run_id)
            .await
            .unwrap_or(false);
        if !succeeded {
            info!(parent_run_id, "Coalesce run did not succeed — skipping weave chain");
            return;
        }

        let already_has_child = crate::db::scout_run::has_child_run(pool, parent_run_id, "weave")
            .await
            .unwrap_or(false);
        if already_has_child {
            info!(parent_run_id, "Weave child already exists — skipping");
            return;
        }

        info!(parent_run_id, "Chaining: coalesce → weave");
        self.run_weave_with_chain(
            region_id,
            scope,
            ChainOpts {
                parent_run_id: Some(parent_run_id.to_string()),
                schedule_id: None,
                chain: false, // weave is the end of the chain
            },
        ).await;
    }

    /// Spawn a scout-source flow: scrape specific sources, with optional region context.
    pub async fn run_scout_source(
        &self,
        source_ids: &[String],
        sources: Vec<rootsignal_common::SourceNode>,
        region: Option<rootsignal_common::RegionNode>,
    ) {
        let deps = self.deps.clone();
        let source_ids_owned: Vec<String> = source_ids.to_vec();
        let run_id = uuid::Uuid::new_v4();
        let budget = crate::db::models::budget::effective_budget(
            &deps.pg_pool, deps.daily_budget_cents,
        ).await;
        info!(source_count = source_ids_owned.len(), %run_id, "Spawning scout-source flow");

        tokio::spawn(async move {
            let run_scope = RunScope::Sources {
                sources: sources.clone(),
                region: region.as_ref().map(ScoutScope::from),
            };
            let engine = deps.build_source_engine(region.as_ref(), run_id);
            let result = engine
                .emit(LifecycleEvent::ScoutRunRequested {
                    run_id,
                    scope: run_scope,
                    budget_cents: budget,
                    region_id: None,
                    flow_type: "scout_source".into(),
                    source_ids: Some(source_ids_owned.clone()),
                    task_id: std::env::var("FLY_MACHINE_ID").ok(),
                    parent_run_id: None,
                    schedule_id: None,
                    run_at: None,
                })
                .correlation_id(run_id)
                .settled()
                .await;

            if let Err(e) = result {
                warn!(error = %e, "Scout-source flow failed");
            } else {
                info!("Scout-source flow completed");
            }
        });
    }

    /// Cancel a running run via Postgres.
    pub async fn cancel_run(&self, run_id: &str) -> bool {
        let run_uuid = match uuid::Uuid::parse_str(run_id) {
            Ok(u) => u,
            Err(_) => return false,
        };

        match sqlx::query(
            "INSERT INTO seesaw_cancellations (correlation_id) VALUES ($1) ON CONFLICT DO NOTHING",
        )
        .bind(run_uuid)
        .execute(&self.deps.pg_pool)
        .await
        {
            Ok(r) => {
                if r.rows_affected() > 0 {
                    info!(run_id, "Cancellation inserted");
                    true
                } else {
                    false
                }
            }
            Err(e) => {
                warn!(run_id, error = %e, "Failed to insert cancellation");
                false
            }
        }
    }

    /// Resume runs that were in-flight when the server crashed.
    ///
    /// Queries `runs` for rows without `finished_at`, reclaims stale
    /// queue entries, rebuilds engines, and calls `settle()` to finish them.
    pub async fn resume_incomplete_runs(&self) {
        let rows = match sqlx::query_as::<_, IncompleteRun>(
            "SELECT run_id, scope, flow_type FROM runs WHERE finished_at IS NULL AND started_at > now() - interval '30 minutes'",
        )
        .fetch_all(&self.deps.pg_pool)
        .await
        {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, "Failed to query incomplete runs");
                return;
            }
        };

        if rows.is_empty() {
            info!("No incomplete runs to resume");
            return;
        }

        info!(count = rows.len(), "Found incomplete runs to resume");

        cleanup_old_cancellations(&self.deps.pg_pool).await;

        for run in rows {
            let run_id = match uuid::Uuid::parse_str(&run.run_id) {
                Ok(u) => u,
                Err(_) => continue,
            };

            let store = PostgresStore::new(self.deps.pg_pool.clone(), run_id);

            if let Err(e) = store.reclaim_stale().await {
                warn!(%run_id, error = %e, "Failed to reclaim stale work");
                continue;
            }

            match store.has_pending_work().await {
                Ok(false) => {
                    info!(%run_id, "No pending work, marking finished");
                    let _ = sqlx::query(
                        "UPDATE runs SET finished_at = now() WHERE run_id = $1 AND finished_at IS NULL",
                    )
                    .bind(run_id.to_string())
                    .execute(&self.deps.pg_pool)
                    .await;
                    continue;
                }
                Err(e) => {
                    warn!(%run_id, error = %e, "Failed to check pending work");
                    continue;
                }
                Ok(true) => {}
            }

            let scope: ScoutScope = match run.scope {
                Some(v) => match serde_json::from_value(v) {
                    Ok(s) => s,
                    Err(e) => {
                        warn!(%run_id, error = %e, "Failed to deserialize scope");
                        continue;
                    }
                },
                None => {
                    warn!(%run_id, "No scope stored, cannot resume");
                    continue;
                }
            };

            let store_arc = Arc::new(store);
            let deps = self.deps.clone();
            let flow_type = run.flow_type.unwrap_or_else(|| "scrape".to_string());

            info!(%run_id, %flow_type, "Resuming incomplete run");

            tokio::spawn(async move {
                let engine_deps = deps.build_engine_deps_for_resume(
                    &scope,
                    run_id,
                );
                let engine = match flow_type.as_str() {
                    "weave" => engine::build_weave_engine(engine_deps, Some(store_arc)),
                    "coalesce" => engine::build_coalesce_engine(engine_deps, Some(store_arc)),
                    _ => engine::build_engine(engine_deps, Some(store_arc)),
                };

                if let Err(e) = engine.settle().await {
                    warn!(%run_id, %flow_type, error = %e, "Resume settle failed");
                } else {
                    info!(%run_id, %flow_type, "Resumed run completed");
                }
            });
        }
    }

    /// Poll `scheduled_scrapes` for due items and trigger runs.
    pub async fn process_scheduled_scrapes(&self, graph: &rootsignal_graph::GraphStore) {
        let rows = match sqlx::query_as::<_, ScheduledScrapeRow>(
            "SELECT id, scope_type, scope_data \
             FROM scheduled_scrapes \
             WHERE completed_at IS NULL AND run_after <= now() \
             ORDER BY run_after ASC \
             LIMIT 10",
        )
        .fetch_all(&self.deps.pg_pool)
        .await
        {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, "Failed to query scheduled scrapes");
                return;
            }
        };

        if rows.is_empty() {
            return;
        }

        info!(count = rows.len(), "Processing due scheduled scrapes");

        for row in rows {
            let triggered = match row.scope_type.as_str() {
                "sources" => {
                    self.trigger_source_scrape(&row.scope_data, graph).await
                }
                "region" => {
                    let region_name = row.scope_data.as_str().unwrap_or_default();
                    info!(region = region_name, "Region scheduled scrape — not yet implemented");
                    true
                }
                other => {
                    warn!(scope_type = other, "Unknown scheduled scrape scope type");
                    true
                }
            };

            if triggered {
                if let Err(e) = sqlx::query(
                    "UPDATE scheduled_scrapes SET completed_at = now() WHERE id = $1",
                )
                .bind(row.id)
                .execute(&self.deps.pg_pool)
                .await
                {
                    warn!(error = %e, "Failed to mark scheduled scrape completed");
                }
            }
        }
    }

    async fn trigger_source_scrape(
        &self,
        scope_data: &serde_json::Value,
        graph: &rootsignal_graph::GraphStore,
    ) -> bool {
        let source_id_strings: Vec<String> = match serde_json::from_value(scope_data.clone()) {
            Ok(ids) => ids,
            Err(e) => {
                warn!(error = %e, "Failed to parse source IDs from scheduled scrape");
                return true;
            }
        };

        let uuids: Vec<uuid::Uuid> = source_id_strings
            .iter()
            .filter_map(|id| uuid::Uuid::parse_str(id).ok())
            .collect();

        let sources = match graph.get_sources_by_ids(&uuids).await {
            Ok(s) => s,
            Err(e) => {
                warn!(error = %e, "Failed to load sources for scheduled scrape");
                return false;
            }
        };

        if sources.is_empty() {
            warn!("Scheduled scrape: no valid sources found, skipping");
            return true;
        }

        for sid in &source_id_strings {
            if crate::db::scout_run::is_source_busy(&self.deps.pg_pool, sid)
                .await
                .unwrap_or(false)
            {
                info!(source_id = sid.as_str(), "Scheduled scrape deferred — source busy");
                return false;
            }
        }

        let region = graph
            .get_region_for_source(&source_id_strings[0])
            .await
            .unwrap_or(None);

        info!(
            source_count = sources.len(),
            "Triggering scheduled source scrape"
        );

        self.run_scout_source(&source_id_strings, sources, region).await;
        true
    }

    /// Start a background loop that checks for due scheduled scrapes.
    pub fn start_scheduled_scrapes_loop(
        self,
        graph: rootsignal_graph::GraphStore,
    ) {
        tokio::spawn(async move {
            let interval = std::time::Duration::from_secs(15 * 60);
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;

            loop {
                self.process_scheduled_scrapes(&graph).await;
                tokio::time::sleep(interval).await;
            }
        });
    }

    /// Poll `schedules` table for due recurring schedules and trigger runs.
    ///
    /// Uses `FOR UPDATE SKIP LOCKED` claim-process-complete cycle.
    /// Emits `ScheduleTriggered` before the entry event to prevent duplicate
    /// runs on partial failure (transactional outbox ordering).
    pub async fn process_schedules(&self, graph: &rootsignal_graph::GraphStore) {
        let rows = match sqlx::query_as::<_, ScheduleRow>(
            "SELECT schedule_id, flow_type, scope, cadence_seconds, region_id \
             FROM schedules \
             WHERE enabled = true \
               AND deleted_at IS NULL \
               AND next_run_at <= now() \
             ORDER BY next_run_at ASC \
             FOR UPDATE SKIP LOCKED \
             LIMIT 20",
        )
        .fetch_all(&self.deps.pg_pool)
        .await
        {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, "Failed to query due schedules");
                return;
            }
        };

        if rows.is_empty() {
            return;
        }

        info!(count = rows.len(), "Processing due schedules");

        for row in rows {
            let run_id = uuid::Uuid::new_v4();

            // 1. Emit ScheduleTriggered FIRST — advances next_run_at to prevent duplicates
            let engine = match self.deps.build_infra_engine(run_id) {
                Some(e) => e,
                None => {
                    warn!("Cannot build infra engine for schedule trigger");
                    continue;
                }
            };

            if let Err(e) = engine
                .emit(rootsignal_scout::domains::scheduling::events::SchedulingEvent::ScheduleTriggered {
                    schedule_id: row.schedule_id.clone(),
                    run_id: run_id.to_string(),
                })
                .correlation_id(run_id)
                .settled()
                .await
            {
                warn!(schedule_id = row.schedule_id.as_str(), error = %e, "Failed to emit ScheduleTriggered");
                continue;
            }

            // 2. Then trigger the actual run based on flow_type
            let should_chain = row.scope.get("chain").and_then(|v| v.as_bool()).unwrap_or(false);
            let chain_opts = ChainOpts {
                parent_run_id: None,
                schedule_id: Some(row.schedule_id.clone()),
                chain: should_chain,
            };

            match row.flow_type.as_str() {
                "scrape" | "scout_source" => {
                    self.trigger_schedule_scrape(&row, graph, run_id, &row.schedule_id).await;
                }
                "weave" => {
                    if let Some(region) = self.resolve_region(&row.region_id, graph).await {
                        let scope = ScoutScope::from(&region);
                        self.run_weave_with_chain(&region.id.to_string(), &scope, chain_opts).await;
                    } else {
                        warn!(schedule_id = row.schedule_id.as_str(), "Weave schedule has no valid region");
                    }
                }
                "coalesce" => {
                    if let Some(region) = self.resolve_region(&row.region_id, graph).await {
                        let scope = ScoutScope::from(&region);
                        self.run_coalesce_for_region(&region.id.to_string(), &scope, chain_opts).await;
                    } else {
                        warn!(schedule_id = row.schedule_id.as_str(), "Coalesce schedule has no valid region");
                    }
                }
                "bootstrap" => {
                    if let Some(region) = self.resolve_region(&row.region_id, graph).await {
                        let scope = ScoutScope::from(&region);
                        self.run_bootstrap(&region.id.to_string(), &scope).await;
                    } else {
                        warn!(schedule_id = row.schedule_id.as_str(), "Bootstrap schedule has no valid region");
                    }
                }
                other => {
                    warn!(schedule_id = row.schedule_id.as_str(), flow_type = other, "Unknown schedule flow_type");
                }
            }
        }
    }

    async fn trigger_schedule_scrape(
        &self,
        row: &ScheduleRow,
        graph: &rootsignal_graph::GraphStore,
        _run_id: uuid::Uuid,
        schedule_id: &str,
    ) {
        // Parse source_ids from scope
        let source_id_strings: Vec<String> = row.scope["source_ids"]
            .as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        if source_id_strings.is_empty() {
            warn!(schedule_id, "Schedule scrape has no source_ids in scope");
            return;
        }

        let uuids: Vec<uuid::Uuid> = source_id_strings
            .iter()
            .filter_map(|id| uuid::Uuid::parse_str(id).ok())
            .collect();

        let sources = match graph.get_sources_by_ids(&uuids).await {
            Ok(s) => s,
            Err(e) => {
                warn!(schedule_id, error = %e, "Failed to load sources for schedule");
                return;
            }
        };

        if sources.is_empty() {
            warn!(schedule_id, "No valid sources found for schedule");
            return;
        }

        for sid in &source_id_strings {
            if crate::db::scout_run::is_source_busy(&self.deps.pg_pool, sid)
                .await
                .unwrap_or(false)
            {
                info!(schedule_id, source_id = sid.as_str(), "Schedule deferred — source busy");
                return;
            }
        }

        let region = graph
            .get_region_for_source(&source_id_strings[0])
            .await
            .unwrap_or(None);

        self.run_scout_source(&source_id_strings, sources, region).await;
    }

    async fn resolve_region(
        &self,
        region_id: &Option<String>,
        graph: &rootsignal_graph::GraphStore,
    ) -> Option<rootsignal_common::RegionNode> {
        let id = region_id.as_deref()?;
        graph.get_region(id).await.ok().flatten()
    }

    /// Start a unified background loop that processes both scheduled scrapes and recurring schedules.
    pub fn start_schedule_loop(
        self,
        graph: rootsignal_graph::GraphStore,
    ) {
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;

            loop {
                // Process legacy one-shot scheduled scrapes
                self.process_scheduled_scrapes(&graph).await;
                // Process recurring schedules
                self.process_schedules(&graph).await;

                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            }
        });
    }
}

#[derive(sqlx::FromRow)]
struct ScheduledScrapeRow {
    id: uuid::Uuid,
    scope_type: String,
    scope_data: serde_json::Value,
}

#[derive(sqlx::FromRow)]
struct ScheduleRow {
    schedule_id: String,
    flow_type: String,
    scope: serde_json::Value,
    #[allow(dead_code)]
    cadence_seconds: i32,
    region_id: Option<String>,
}

#[derive(sqlx::FromRow)]
struct IncompleteRun {
    run_id: String,
    scope: Option<serde_json::Value>,
    flow_type: Option<String>,
}

/// Remove cancellation rows older than 7 days. Called on startup during resume.
async fn cleanup_old_cancellations(pool: &sqlx::PgPool) {
    match sqlx::query(
        "DELETE FROM seesaw_cancellations WHERE cancelled_at < now() - interval '7 days'",
    )
    .execute(pool)
    .await
    {
        Ok(r) => {
            if r.rows_affected() > 0 {
                info!(deleted = r.rows_affected(), "Cleaned up old cancellation rows");
            }
        }
        Err(e) => {
            warn!(error = %e, "Failed to clean up old cancellations");
        }
    }
}

