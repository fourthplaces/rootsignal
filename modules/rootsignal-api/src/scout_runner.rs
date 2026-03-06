//! Spawns seesaw engines in tokio tasks. Each GraphQL mutation creates a
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
use seesaw_core::store::Store;
use rootsignal_scout::domains::lifecycle::events::LifecycleEvent;
use rootsignal_scout::workflows::ScoutDeps;

/// Holds `Arc<ScoutDeps>`. Spawns seesaw engines in tokio tasks.
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
        let run_id = uuid::Uuid::new_v4().to_string();

        info!("Spawning news scan");

        tokio::spawn(async move {
            let engine = deps.build_news_engine(&run_id);

            let run_id_uuid = uuid::Uuid::parse_str(&run_id).unwrap();
            if let Err(e) = engine
                .emit(LifecycleEvent::NewsScanRequested)
                .correlation_id(run_id_uuid)
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
        let run_id = uuid::Uuid::new_v4().to_string();

        info!(region_id = region_id.as_str(), run_id = run_id.as_str(), "Spawning bootstrap flow");

        tokio::spawn(async move {
            let run_id_uuid = uuid::Uuid::parse_str(&run_id).unwrap();

            early_insert_flow_run(&deps, &run_id, Some(&region_id), "bootstrap", None, &scope).await;

            let engine = deps.build_scrape_engine(&scope, &run_id, None);
            let result = engine
                .emit(LifecycleEvent::ScoutRunRequested { run_id: run_id.clone() })
                .correlation_id(run_id_uuid)
                .settled()
                .await;

            if let Err(e) = result {
                warn!(region_id = region_id.as_str(), error = %e, "Bootstrap flow failed");
            } else {
                info!(region_id = region_id.as_str(), "Bootstrap flow completed");
            }

            post_settle_cleanup(&deps, run_id_uuid, None).await;
        });
    }

    /// Spawn a scrape flow for a region: auto-bootstraps if no sources, then scrapes + extracts.
    pub async fn run_scrape(&self, region_id: &str, scope: &ScoutScope) {
        let deps = self.deps.clone();
        let region_id = region_id.to_string();
        let scope = scope.clone();
        let run_id = uuid::Uuid::new_v4().to_string();

        info!(region_id = region_id.as_str(), run_id = run_id.as_str(), "Spawning scrape flow");

        tokio::spawn(async move {
            let run_id_uuid = uuid::Uuid::parse_str(&run_id).unwrap();

            early_insert_flow_run(&deps, &run_id, Some(&region_id), "scrape", None, &scope).await;

            let engine = deps.build_scrape_engine(&scope, &run_id, None);
            let result = engine
                .emit(LifecycleEvent::ScoutRunRequested { run_id: run_id.clone() })
                .correlation_id(run_id_uuid)
                .settled()
                .await;

            if let Err(e) = result {
                warn!(region_id = region_id.as_str(), error = %e, "Scrape flow failed");
            } else {
                info!(region_id = region_id.as_str(), "Scrape flow completed");
            }

            post_settle_cleanup(&deps, run_id_uuid, None).await;
        });
    }

    /// Spawn a weave flow for a region: cross-signal synthesis at any level.
    pub async fn run_weave(&self, region_id: &str, scope: &ScoutScope) {
        let deps = self.deps.clone();
        let region_id = region_id.to_string();
        let scope = scope.clone();
        let run_id = uuid::Uuid::new_v4().to_string();

        info!(region_id = region_id.as_str(), run_id = run_id.as_str(), "Spawning weave flow");

        tokio::spawn(async move {
            let run_id_uuid = uuid::Uuid::parse_str(&run_id).unwrap();

            early_insert_flow_run(&deps, &run_id, Some(&region_id), "weave", None, &scope).await;

            // Weave engine starts from synthesis phase (skips scrape chain)
            let engine = deps.build_weave_engine(&scope, &run_id, None);
            let result = engine
                .emit(LifecycleEvent::ScoutRunRequested { run_id: run_id.clone() })
                .correlation_id(run_id_uuid)
                .settled()
                .await;

            if let Err(e) = result {
                warn!(region_id = region_id.as_str(), error = %e, "Weave flow failed");
            } else {
                info!(region_id = region_id.as_str(), "Weave flow completed");
            }

            post_settle_cleanup(&deps, run_id_uuid, None).await;
        });
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
        let run_id = uuid::Uuid::new_v4().to_string();
        let metadata_scope = region.as_ref()
            .map(ScoutScope::from)
            .unwrap_or(ScoutScope {
                name: format!("sources:{}", source_ids.len()),
                center_lat: 0.0,
                center_lng: 0.0,
                radius_km: 0.0,
            });

        info!(source_count = source_ids_owned.len(), run_id = run_id.as_str(), "Spawning scout-source flow");

        tokio::spawn(async move {
            let run_id_uuid = uuid::Uuid::parse_str(&run_id).unwrap();
            let source_ids_json = serde_json::to_value(&source_ids_owned).ok();

            early_insert_flow_run(&deps, &run_id, None, "scout_source", source_ids_json.as_ref(), &metadata_scope).await;

            let engine = deps.build_source_engine(region.as_ref(), &run_id, sources);
            let result = engine
                .emit(LifecycleEvent::ScoutRunRequested { run_id: run_id.clone() })
                .correlation_id(run_id_uuid)
                .settled()
                .await;

            if let Err(e) = result {
                warn!(error = %e, "Scout-source flow failed");
            } else {
                info!("Scout-source flow completed");
            }

            post_settle_cleanup(&deps, run_id_uuid, None).await;
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
    /// Queries `scout_runs` for rows without `finished_at`, reclaims stale
    /// queue entries, rebuilds engines, and calls `settle()` to finish them.
    pub async fn resume_incomplete_runs(&self) {
        let rows = match sqlx::query_as::<_, IncompleteRun>(
            "SELECT run_id, task_id, scope FROM scout_runs WHERE finished_at IS NULL",
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
            let run_uuid = match uuid::Uuid::parse_str(&run.run_id) {
                Ok(u) => u,
                Err(_) => continue,
            };

            let store = PostgresStore::new(self.deps.pg_pool.clone(), run_uuid);

            if let Err(e) = store.reclaim_stale().await {
                warn!(run_id = %run.run_id, error = %e, "Failed to reclaim stale work");
                continue;
            }

            match store.has_pending_work().await {
                Ok(false) => {
                    info!(run_id = %run.run_id, "No pending work, marking finished");
                    let _ = sqlx::query(
                        "UPDATE scout_runs SET finished_at = now() WHERE run_id = $1 AND finished_at IS NULL",
                    )
                    .bind(&run.run_id)
                    .execute(&self.deps.pg_pool)
                    .await;
                    continue;
                }
                Err(e) => {
                    warn!(run_id = %run.run_id, error = %e, "Failed to check pending work");
                    continue;
                }
                Ok(true) => {}
            }

            let scope: ScoutScope = match run.scope {
                Some(v) => match serde_json::from_value(v) {
                    Ok(s) => s,
                    Err(e) => {
                        warn!(run_id = %run.run_id, error = %e, "Failed to deserialize scope");
                        continue;
                    }
                },
                None => {
                    warn!(run_id = %run.run_id, "No scope stored, cannot resume");
                    continue;
                }
            };

            let store_arc = Arc::new(store) as Arc<dyn seesaw_core::Store>;
            let deps = self.deps.clone();
            let run_id = run.run_id.clone();
            let task_id = run.task_id.clone();

            info!(run_id = %run_id, "Resuming incomplete run");

            tokio::spawn(async move {
                let run_uuid = uuid::Uuid::parse_str(&run_id).unwrap();
                let engine_deps = deps.build_engine_deps_for_resume(
                    &scope,
                    &run_id,
                    task_id.as_deref(),
                );
                let engine = engine::build_full_engine(engine_deps, Some(store_arc));

                if let Err(e) = engine.settle().await {
                    warn!(run_id = %run_id, error = %e, "Resume settle failed");
                } else {
                    info!(run_id = %run_id, "Resumed run completed");
                }

                post_settle_cleanup(&deps, run_uuid, task_id.as_deref()).await;
            });
        }
    }
}

#[derive(sqlx::FromRow)]
struct IncompleteRun {
    run_id: String,
    task_id: Option<String>,
    scope: Option<serde_json::Value>,
}

/// INSERT scout_runs row with flow metadata before settle.
async fn early_insert_flow_run(
    deps: &ScoutDeps,
    run_id: &str,
    region_id: Option<&str>,
    flow_type: &str,
    source_ids: Option<&serde_json::Value>,
    scope: &ScoutScope,
) {
    let scope_json = serde_json::to_value(scope).ok();
    if let Err(e) = sqlx::query(
        "INSERT INTO scout_runs (run_id, region, region_id, flow_type, source_ids, scope, started_at) \
         VALUES ($1, $2, $3, $4, $5, $6, now()) \
         ON CONFLICT (run_id) DO NOTHING",
    )
    .bind(run_id)
    .bind(&scope.name)
    .bind(region_id)
    .bind(flow_type)
    .bind(source_ids)
    .bind(&scope_json)
    .execute(&deps.pg_pool)
    .await
    {
        warn!(run_id, error = %e, "Failed to early-insert scout_runs row");
    }
}

/// Post-settle cleanup: mark scout_runs.finished_at.
async fn post_settle_cleanup(deps: &ScoutDeps, run_id_uuid: uuid::Uuid, _task_id: Option<&str>) {
    if let Err(e) = sqlx::query(
        "UPDATE scout_runs SET finished_at = now() WHERE run_id = $1 AND finished_at IS NULL",
    )
    .bind(run_id_uuid.to_string())
    .execute(&deps.pg_pool)
    .await
    {
        warn!(run_id = %run_id_uuid, error = %e, "Failed to mark scout_run finished");
    }
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

