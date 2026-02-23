use chrono::{DateTime, Duration, Utc};
use neo4rs::query;
use tracing::info;
use uuid::Uuid;

use rootsignal_graph::GraphClient;

/// Manages the SupervisorState node (watermark + calibrated thresholds).
pub struct SupervisorState {
    client: GraphClient,
    region: String,
}

impl SupervisorState {
    pub fn new(client: GraphClient, region: String) -> Self {
        Self { client, region }
    }

    /// Read the last_run watermark. Returns None if no state exists (first boot).
    pub async fn last_run(&self) -> Result<Option<DateTime<Utc>>, neo4rs::Error> {
        let q = query(
            "MATCH (s:SupervisorState)
             WHERE s.region = $region
             RETURN s.last_run AS last_run",
        )
        .param("region", self.region.clone());

        let mut stream = self.client.inner().execute(q).await?;
        if let Some(row) = stream.next().await? {
            let s: String = row.get("last_run").unwrap_or_default();
            if s.is_empty() {
                return Ok(None);
            }
            let dt = chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M:%S%.f")
                .ok()
                .map(|ndt| ndt.and_utc());
            Ok(dt)
        } else {
            Ok(None)
        }
    }

    /// Compute the effective watermark window for this run.
    /// Returns (from, to) where from = last_run (or now-24h) and to = min(now, from+24h).
    pub async fn watermark_window(&self) -> Result<(DateTime<Utc>, DateTime<Utc>), neo4rs::Error> {
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

    /// Update the last_run watermark. Creates the state node if it doesn't exist.
    pub async fn update_last_run(&self, dt: &DateTime<Utc>) -> Result<(), neo4rs::Error> {
        let ts = rootsignal_graph::writer::format_datetime_pub(dt);

        let q = query(
            "MERGE (s:SupervisorState {region: $region})
             ON CREATE SET s.id = $id,
                           s.last_run = datetime($last_run),
                           s.min_confidence = 0.0,
                           s.dedup_threshold_recommendation = 0.92,
                           s.version = 1
             ON MATCH SET s.last_run = datetime($last_run)",
        )
        .param("region", self.region.clone())
        .param("id", Uuid::new_v4().to_string())
        .param("last_run", ts);

        self.client.inner().run(q).await?;
        Ok(())
    }

    /// Acquire a supervisor lock. Returns false if another supervisor is running.
    /// Cleans up stale locks (>30 min) from killed containers.
    pub async fn acquire_lock(&self) -> Result<bool, neo4rs::Error> {
        // Delete stale locks older than 30 minutes
        self.client
            .inner()
            .run(query(
                "MATCH (lock:SupervisorLock) WHERE lock.started_at < datetime() - duration('PT30M') DELETE lock"
            ))
            .await?;

        // Atomic check-and-create
        let q = query(
            "OPTIONAL MATCH (existing:SupervisorLock)
             WITH existing WHERE existing IS NULL
             CREATE (lock:SupervisorLock {started_at: datetime()})
             RETURN lock IS NOT NULL AS acquired",
        );

        let mut result = self.client.inner().execute(q).await?;
        if let Some(row) = result.next().await? {
            let acquired: bool = row.get("acquired").unwrap_or(false);
            return Ok(acquired);
        }

        Ok(false)
    }

    /// Release the supervisor lock.
    pub async fn release_lock(&self) -> Result<(), neo4rs::Error> {
        self.client
            .inner()
            .run(query("MATCH (lock:SupervisorLock) DELETE lock"))
            .await?;
        Ok(())
    }

    /// Check if any scout is currently running (RegionScoutRun with running_* status).
    pub async fn is_scout_running(&self) -> Result<bool, neo4rs::Error> {
        let q = query(
            "MATCH (r:RegionScoutRun) \
             WHERE r.status STARTS WITH 'running_' \
               AND r.updated_at >= datetime() - duration('PT30M') \
             RETURN count(r) > 0 AS running"
        );
        let mut stream = self.client.inner().execute(q).await?;
        if let Some(row) = stream.next().await? {
            let running: bool = row.get("running").unwrap_or(false);
            return Ok(running);
        }
        Ok(false)
    }
}
