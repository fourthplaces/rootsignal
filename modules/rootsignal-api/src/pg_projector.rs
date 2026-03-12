//! Postgres read-model projector for replay.
//!
//! Owns the `runs` and `scheduled_scrapes` projected tables in a
//! separate database. `prepare()` creates the database if needed, ensures
//! the schema, and truncates for a clean rebuild. `project_batch()` then
//! populates them by matching on `event_type` strings from `PersistedEvent`.

use anyhow::Result;
use causal::types::PersistedEvent;
use sqlx::PgPool;
use tracing::info;

/// DDL for projection-owned tables. These live here — not in rootsignal-migrate —
/// because the projection DB is a derived read model that replay owns entirely.
const SCHEMA_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS runs (
    run_id        TEXT        PRIMARY KEY,
    region        TEXT        NOT NULL,
    started_at    TIMESTAMPTZ NOT NULL,
    finished_at   TIMESTAMPTZ,
    stats         JSONB       NOT NULL DEFAULT '{}',
    region_id     TEXT,
    flow_type     TEXT,
    source_ids    JSONB,
    task_id       TEXT,
    scope         JSONB,
    spent_cents   BIGINT      DEFAULT 0,
    parent_run_id TEXT,
    schedule_id   TEXT,
    run_at        TIMESTAMPTZ,
    error         TEXT,
    cancelled_at  TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_runs_region_finished
    ON runs (region, finished_at DESC);
CREATE INDEX IF NOT EXISTS idx_runs_region_id
    ON runs (region_id) WHERE region_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_runs_flow_type
    ON runs (flow_type) WHERE flow_type IS NOT NULL;

CREATE TABLE IF NOT EXISTS scheduled_scrapes (
    id           UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    scope_type   TEXT        NOT NULL,
    scope_data   JSONB       NOT NULL,
    run_after    TIMESTAMPTZ NOT NULL,
    reason       TEXT        NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_scheduled_scrapes_pending
    ON scheduled_scrapes (run_after)
    WHERE completed_at IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_scheduled_scrapes_unique_pending
    ON scheduled_scrapes (scope_type, scope_data)
    WHERE completed_at IS NULL;
"#;

pub struct PostgresProjector {
    pool: PgPool,
}

impl PostgresProjector {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Prepare the projection database for a clean rebuild.
    ///
    /// Creates tables if they don't exist, then truncates everything.
    /// Call this once before replay starts.
    pub async fn prepare(&self) -> Result<()> {
        sqlx::raw_sql(SCHEMA_DDL).execute(&self.pool).await?;
        sqlx::query("TRUNCATE runs, scheduled_scrapes")
            .execute(&self.pool)
            .await?;
        info!("Projection database ready (tables created, truncated)");
        Ok(())
    }

    /// Project a batch of events into Postgres read-model tables.
    pub async fn project_batch(&self, events: &[PersistedEvent]) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        for event in events {
            match event.event_type.as_str() {
                "lifecycle:scout_run_requested" => {
                    let p = &event.payload;
                    let run_id = p["run_id"].as_str().unwrap_or_default();
                    let region = extract_region(&p["scope"]);
                    let region_id = p["region_id"].as_str();
                    let flow_type = p["flow_type"].as_str().unwrap_or("");
                    let source_ids = p.get("source_ids").filter(|v| !v.is_null());
                    let scope = p.get("scope").filter(|v| !v.is_null());
                    let task_id = p["task_id"].as_str();
                    let parent_run_id = p["parent_run_id"].as_str();
                    let schedule_id = p["schedule_id"].as_str();
                    let run_at = p["run_at"]
                        .as_str()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .map(|dt| dt.with_timezone(&chrono::Utc));

                    sqlx::query(
                        "INSERT INTO runs (run_id, region, region_id, flow_type, source_ids, scope, task_id, parent_run_id, schedule_id, run_at, started_at) \
                         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, COALESCE($10, $11), $11) \
                         ON CONFLICT (run_id) DO NOTHING",
                    )
                    .bind(run_id)
                    .bind(&region)
                    .bind(region_id)
                    .bind(flow_type)
                    .bind(source_ids)
                    .bind(scope)
                    .bind(task_id)
                    .bind(parent_run_id)
                    .bind(schedule_id)
                    .bind(run_at)
                    .bind(event.created_at)
                    .execute(&mut *tx)
                    .await?;
                }

                "lifecycle:generate_situations_requested" => {
                    let p = &event.payload;
                    let run_id = p["run_id"].as_str().unwrap_or_default();
                    let region = p["region"]["name"].as_str().unwrap_or("unknown");
                    let region_id = p["region_id"].as_str();
                    let task_id = p["task_id"].as_str();
                    let parent_run_id = p["parent_run_id"].as_str();
                    let schedule_id = p["schedule_id"].as_str();
                    let run_at = p["run_at"]
                        .as_str()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .map(|dt| dt.with_timezone(&chrono::Utc));

                    sqlx::query(
                        "INSERT INTO runs (run_id, region, region_id, flow_type, task_id, parent_run_id, schedule_id, run_at, started_at) \
                         VALUES ($1, $2, $3, 'weave', $4, $5, $6, COALESCE($7, $8), $8) \
                         ON CONFLICT (run_id) DO NOTHING",
                    )
                    .bind(run_id)
                    .bind(region)
                    .bind(region_id)
                    .bind(task_id)
                    .bind(parent_run_id)
                    .bind(schedule_id)
                    .bind(run_at)
                    .bind(event.created_at)
                    .execute(&mut *tx)
                    .await?;
                }

                "lifecycle:coalesce_requested" => {
                    let p = &event.payload;
                    let run_id = p["run_id"].as_str().unwrap_or_default();
                    let region = p["region"]["name"].as_str().unwrap_or("unknown");
                    let region_id = p["region_id"].as_str();
                    let task_id = p["task_id"].as_str();
                    let parent_run_id = p["parent_run_id"].as_str();
                    let schedule_id = p["schedule_id"].as_str();
                    let run_at = p["run_at"]
                        .as_str()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .map(|dt| dt.with_timezone(&chrono::Utc));

                    sqlx::query(
                        "INSERT INTO runs (run_id, region, region_id, flow_type, task_id, parent_run_id, schedule_id, run_at, started_at) \
                         VALUES ($1, $2, $3, 'coalesce', $4, $5, $6, COALESCE($7, $8), $8) \
                         ON CONFLICT (run_id) DO NOTHING",
                    )
                    .bind(run_id)
                    .bind(region)
                    .bind(region_id)
                    .bind(task_id)
                    .bind(parent_run_id)
                    .bind(schedule_id)
                    .bind(run_at)
                    .bind(event.created_at)
                    .execute(&mut *tx)
                    .await?;
                }

                "lifecycle:run_cancelled" => {
                    let p = &event.payload;
                    let run_id = p["run_id"].as_str().unwrap_or_default();
                    let cancelled_at = p["cancelled_at"]
                        .as_str()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .map(|dt| dt.with_timezone(&chrono::Utc));

                    sqlx::query(
                        "UPDATE runs SET cancelled_at = $2, finished_at = $2 \
                         WHERE run_id = $1 AND finished_at IS NULL",
                    )
                    .bind(run_id)
                    .bind(cancelled_at)
                    .execute(&mut *tx)
                    .await?;
                }

                "lifecycle:run_failed" => {
                    let p = &event.payload;
                    let run_id = p["run_id"].as_str().unwrap_or_default();
                    let error = p["error"].as_str().unwrap_or_default();

                    sqlx::query(
                        "UPDATE runs SET error = $2, finished_at = $3 \
                         WHERE run_id = $1 AND finished_at IS NULL",
                    )
                    .bind(run_id)
                    .bind(error)
                    .bind(event.created_at)
                    .execute(&mut *tx)
                    .await?;
                }

                "lifecycle:scout_run_completed" => {
                    let p = &event.payload;
                    let run_id = p["run_id"].as_str().unwrap_or_default();
                    let finished_at = p["finished_at"]
                        .as_str()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .map(|dt| dt.with_timezone(&chrono::Utc));

                    sqlx::query(
                        "UPDATE runs SET finished_at = $2 WHERE run_id = $1 AND finished_at IS NULL",
                    )
                    .bind(run_id)
                    .bind(finished_at)
                    .execute(&mut *tx)
                    .await?;
                }

                "scheduling:scrape_scheduled" => {
                    let p = &event.payload;
                    let scope = &p["scope"];
                    let scope_type = scope["scope_type"].as_str().unwrap_or("unknown");
                    let scope_data = match scope_type {
                        "sources" => &scope["source_ids"],
                        "region" => &scope["region"],
                        _ => &serde_json::Value::Null,
                    };
                    let run_after = p["run_after"]
                        .as_str()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .map(|dt| dt.with_timezone(&chrono::Utc));
                    let reason = p["reason"].as_str().unwrap_or("");

                    if let Some(run_after) = run_after {
                        sqlx::query(
                            "INSERT INTO scheduled_scrapes (scope_type, scope_data, run_after, reason) \
                             VALUES ($1, $2, $3, $4) \
                             ON CONFLICT DO NOTHING",
                        )
                        .bind(scope_type)
                        .bind(scope_data)
                        .bind(run_after)
                        .bind(reason)
                        .execute(&mut *tx)
                        .await?;
                    }
                }

                _ => {}
            }
        }

        tx.commit().await?;
        Ok(())
    }
}

/// Extract region name from a RunScope JSON payload.
///
/// RunScope serializes as `{"type": "Region", "name": "twincities", ...}` or
/// `{"type": "Sources", "region": {"name": "twincities", ...}, ...}`.
fn extract_region(scope: &serde_json::Value) -> String {
    match scope["type"].as_str() {
        Some("Region") => scope["name"].as_str().unwrap_or("unknown").to_string(),
        Some("Sources") => scope["region"]["name"]
            .as_str()
            .or_else(|| scope["sources"][0]["canonical_key"].as_str())
            .unwrap_or("unknown")
            .to_string(),
        _ => "unknown".to_string(),
    }
}

/// Connect to the projection database, creating it if necessary.
///
/// 1. Resolves the projection URL (explicit env var or derived from DATABASE_URL)
/// 2. Connects to the server's maintenance DB to CREATE DATABASE IF NOT EXISTS
/// 3. Connects to the projection DB and returns a ready-to-use PostgresProjector
pub async fn connect() -> Result<PostgresProjector> {
    let projection_url = match std::env::var("PROJECTION_DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            let main_url = std::env::var("DATABASE_URL")
                .expect("DATABASE_URL required for replay");
            derive_projection_url(&main_url)
        }
    };

    let (db_name, maintenance_url) = split_db_url(&projection_url);
    info!(db = db_name.as_str(), "Ensuring projection database exists");

    let maintenance_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&maintenance_url)
        .await?;
    // Safe: db_name is derived from the URL, not user input.
    let create_sql = format!("CREATE DATABASE \"{}\"", db_name.replace('"', "\"\""));
    match sqlx::query(&create_sql).execute(&maintenance_pool).await {
        Ok(_) => info!(db = db_name.as_str(), "Created projection database"),
        Err(e) if e.to_string().contains("already exists") => {}
        Err(e) => return Err(e.into()),
    }
    maintenance_pool.close().await;

    info!(url = projection_url.as_str(), "Connecting to projection database");
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(3)
        .connect(&projection_url)
        .await?;

    Ok(PostgresProjector::new(pool))
}

/// Derive a projection database URL from the main DATABASE_URL by appending `_projection`
/// to the database name.
pub fn derive_projection_url(database_url: &str) -> String {
    let (base, db_and_query) = split_at_last_slash(database_url);
    let (db_name, query) = db_and_query.split_once('?').unwrap_or((db_and_query, ""));
    let suffix = if query.is_empty() {
        String::new()
    } else {
        format!("?{query}")
    };
    format!("{base}{db_name}_projection{suffix}")
}

/// Split a Postgres URL into (database_name, maintenance_url pointing at `postgres` db).
fn split_db_url(url: &str) -> (String, String) {
    let (base, db_and_query) = split_at_last_slash(url);
    let (db_name, query) = db_and_query.split_once('?').unwrap_or((db_and_query, ""));
    let suffix = if query.is_empty() {
        String::new()
    } else {
        format!("?{query}")
    };
    (db_name.to_string(), format!("{base}postgres{suffix}"))
}

fn split_at_last_slash(url: &str) -> (&str, &str) {
    match url.rfind('/') {
        Some(pos) => (&url[..=pos], &url[pos + 1..]),
        None => (url, ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_projection_url_appends_suffix() {
        assert_eq!(
            derive_projection_url("postgres://user:pass@host:5432/rootsignal"),
            "postgres://user:pass@host:5432/rootsignal_projection"
        );
    }

    #[test]
    fn derive_projection_url_preserves_query_params() {
        assert_eq!(
            derive_projection_url("postgres://user:pass@host:5432/mydb?sslmode=require"),
            "postgres://user:pass@host:5432/mydb_projection?sslmode=require"
        );
    }

    #[test]
    fn split_db_url_extracts_name_and_maintenance() {
        let (name, maint) = split_db_url("postgres://user:pass@host:5432/rootsignal_projection");
        assert_eq!(name, "rootsignal_projection");
        assert_eq!(maint, "postgres://user:pass@host:5432/postgres");
    }

    #[test]
    fn split_db_url_preserves_query_params() {
        let (name, maint) = split_db_url("postgres://u:p@h:5432/mydb_projection?sslmode=require");
        assert_eq!(name, "mydb_projection");
        assert_eq!(maint, "postgres://u:p@h:5432/postgres?sslmode=require");
    }

    #[test]
    fn extract_region_from_region_scope() {
        let scope = serde_json::json!({"type": "Region", "name": "twincities", "center_lat": 44.9});
        assert_eq!(extract_region(&scope), "twincities");
    }

    #[test]
    fn extract_region_from_sources_scope() {
        let scope = serde_json::json!({"type": "Sources", "sources": [], "region": {"name": "mpls"}});
        assert_eq!(extract_region(&scope), "mpls");
    }

    #[test]
    fn extract_region_from_sources_with_null_region_falls_back_to_canonical_key() {
        let scope = serde_json::json!({
            "type": "Sources",
            "region": null,
            "sources": [{"canonical_key": "site:linktr.ee mutual aid Minneapolis"}]
        });
        assert_eq!(extract_region(&scope), "site:linktr.ee mutual aid Minneapolis");
    }

    #[test]
    fn extract_region_from_unscoped() {
        let scope = serde_json::json!({"type": "Unscoped"});
        assert_eq!(extract_region(&scope), "unknown");
    }
}
