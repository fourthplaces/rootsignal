use chrono::{DateTime, Duration, Utc};
use neo4rs::query;
use sqlx::PgPool;
use tracing::info;

use rootsignal_graph::GraphClient;

/// Manages supervisor state: advisory lock, watermark (from scout_runs), and scout-running check.
pub struct SupervisorState {
    pg_pool: PgPool,
    client: GraphClient,
    region: String,
    lock_key: i64,
}

impl SupervisorState {
    pub fn new(pg_pool: PgPool, client: GraphClient, region: String) -> Self {
        // Stable hash of region name → advisory lock key
        let lock_key = {
            let mut hash: i64 = 5381;
            for byte in region.as_bytes() {
                hash = hash.wrapping_mul(33).wrapping_add(*byte as i64);
            }
            hash
        };
        Self {
            pg_pool,
            client,
            region,
            lock_key,
        }
    }

    /// Read the last_run watermark. Returns None if no state exists (first boot).
    pub async fn last_run(&self) -> Result<Option<DateTime<Utc>>, anyhow::Error> {
        let row: Option<(DateTime<Utc>,)> = sqlx::query_as(
            "SELECT last_run FROM supervisor_watermarks WHERE region = $1",
        )
        .bind(&self.region)
        .fetch_optional(&self.pg_pool)
        .await?;

        Ok(row.map(|(dt,)| dt))
    }

    /// Update the last_run watermark.
    pub async fn update_last_run(&self, dt: &DateTime<Utc>) -> Result<(), anyhow::Error> {
        sqlx::query(
            "INSERT INTO supervisor_watermarks (region, last_run, updated_at)
             VALUES ($1, $2, now())
             ON CONFLICT (region)
             DO UPDATE SET last_run = $2, updated_at = now()",
        )
        .bind(&self.region)
        .bind(dt)
        .execute(&self.pg_pool)
        .await?;

        Ok(())
    }

    /// Compute the effective watermark window for this run.
    /// Returns (from, to) where from = last_run (or now-24h) and to = min(now, from+24h).
    pub async fn watermark_window(&self) -> Result<(DateTime<Utc>, DateTime<Utc>), anyhow::Error> {
        let now = Utc::now();
        let last = self.last_run().await?;

        let from = match last {
            Some(dt) => dt,
            None => {
                info!("First boot: seeding watermark to now - 24h");
                now - Duration::hours(24)
            }
        };

        // Cap at 24h per run to prevent cost blowout
        let to = (from + Duration::hours(24)).min(now);

        Ok((from, to))
    }

    /// Acquire a supervisor lock via Postgres advisory lock.
    /// Returns false if another supervisor is running.
    pub async fn acquire_lock(&self) -> Result<bool, anyhow::Error> {
        let (acquired,): (bool,) =
            sqlx::query_as("SELECT pg_try_advisory_lock($1)")
                .bind(self.lock_key)
                .fetch_one(&self.pg_pool)
                .await?;
        Ok(acquired)
    }

    /// Release the supervisor lock.
    pub async fn release_lock(&self) -> Result<(), anyhow::Error> {
        sqlx::query("SELECT pg_advisory_unlock($1)")
            .bind(self.lock_key)
            .execute(&self.pg_pool)
            .await?;
        Ok(())
    }

    /// Check if any scout task is currently running (ScoutTask with running_* phase_status).
    pub async fn is_scout_running(&self) -> Result<bool, neo4rs::Error> {
        let q = query(
            "MATCH (t:ScoutTask) \
             WHERE t.phase_status STARTS WITH 'running_' \
               AND t.phase_status_updated_at >= datetime() - duration('PT30M') \
             RETURN count(t) > 0 AS running",
        );
        let mut stream = self.client.execute(q).await?;
        if let Some(row) = stream.next().await? {
            let running: bool = row.get("running").unwrap_or(false);
            return Ok(running);
        }
        Ok(false)
    }
}
