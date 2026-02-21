// Postgres persistence for web interactions. Internal to the archive crate.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tracing::warn;
use uuid::Uuid;

use crate::error::Result;

#[derive(Clone)]
pub(crate) struct ArchiveStore {
    pool: PgPool,
}

/// A row from the web_interactions table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub(crate) struct StoredInteraction {
    pub id: Uuid,
    pub run_id: Uuid,
    pub region_slug: String,
    pub kind: String,
    pub target: String,
    pub target_raw: String,
    pub fetcher: String,
    pub raw_html: Option<String>,
    pub markdown: Option<String>,
    pub response_json: Option<serde_json::Value>,
    pub raw_bytes: Option<Vec<u8>>,
    pub content_hash: String,
    pub fetched_at: DateTime<Utc>,
    pub duration_ms: i32,
    pub error: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub semantics: Option<serde_json::Value>,
}

/// Parameters for inserting a new web interaction.
pub(crate) struct InsertInteraction {
    pub run_id: Uuid,
    pub region_slug: String,
    pub kind: String,
    pub target: String,
    pub target_raw: String,
    pub fetcher: String,
    pub raw_html: Option<String>,
    pub markdown: Option<String>,
    pub response_json: Option<serde_json::Value>,
    pub raw_bytes: Option<Vec<u8>>,
    pub content_hash: String,
    pub duration_ms: i32,
    pub error: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub semantics: Option<serde_json::Value>,
}

impl ArchiveStore {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Run the embedded SQL migrations.
    pub(crate) async fn migrate(&self) -> Result<()> {
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .map_err(|e| crate::error::ArchiveError::Database(e.into()))?;
        Ok(())
    }

    /// Record a web interaction. Logs a warning on failure rather than propagating —
    /// a failed Postgres write shouldn't abort the scrape.
    pub(crate) async fn insert(&self, i: InsertInteraction) -> Option<Uuid> {
        let result = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO web_interactions
                (run_id, region_slug, kind, target, target_raw, fetcher,
                 raw_html, markdown, response_json, raw_bytes,
                 content_hash, duration_ms, error, metadata, semantics)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
            RETURNING id
            "#,
        )
        .bind(&i.run_id)
        .bind(&i.region_slug)
        .bind(&i.kind)
        .bind(&i.target)
        .bind(&i.target_raw)
        .bind(&i.fetcher)
        .bind(&i.raw_html)
        .bind(&i.markdown)
        .bind(&i.response_json)
        .bind(&i.raw_bytes)
        .bind(&i.content_hash)
        .bind(i.duration_ms)
        .bind(&i.error)
        .bind(&i.metadata)
        .bind(&i.semantics)
        .fetch_one(&self.pool)
        .await;

        match result {
            Ok(id) => Some(id),
            Err(e) => {
                warn!(target = %i.target, error = %e, "Failed to record web interaction");
                None
            }
        }
    }

    /// Most recent interaction for a normalized target.
    pub(crate) async fn latest_by_target(&self, target: &str) -> Result<Option<StoredInteraction>> {
        let row = sqlx::query_as::<_, StoredInteraction>(
            r#"
            SELECT * FROM web_interactions
            WHERE target = $1
            ORDER BY fetched_at DESC
            LIMIT 1
            "#,
        )
        .bind(target)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row)
    }

    /// All snapshots of a target over time.
    pub(crate) async fn history(&self, target: &str) -> Result<Vec<StoredInteraction>> {
        let rows = sqlx::query_as::<_, StoredInteraction>(
            r#"
            SELECT * FROM web_interactions
            WHERE target = $1
            ORDER BY fetched_at DESC
            "#,
        )
        .bind(target)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    /// Everything from a specific run.
    pub(crate) async fn by_run(&self, run_id: Uuid) -> Result<Vec<StoredInteraction>> {
        let rows = sqlx::query_as::<_, StoredInteraction>(
            r#"
            SELECT * FROM web_interactions
            WHERE run_id = $1
            ORDER BY fetched_at ASC
            "#,
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    /// Lookup by content hash.
    pub(crate) async fn by_content_hash(
        &self,
        hash: &str,
    ) -> Result<Option<StoredInteraction>> {
        let row = sqlx::query_as::<_, StoredInteraction>(
            r#"
            SELECT * FROM web_interactions
            WHERE content_hash = $1
            LIMIT 1
            "#,
        )
        .bind(hash)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row)
    }

    /// Region + time range query.
    pub(crate) async fn by_region_and_range(
        &self,
        region: &str,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<StoredInteraction>> {
        let rows = sqlx::query_as::<_, StoredInteraction>(
            r#"
            SELECT * FROM web_interactions
            WHERE region_slug = $1
              AND fetched_at >= $2
              AND fetched_at < $3
            ORDER BY fetched_at DESC
            "#,
        )
        .bind(region)
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    /// Most recent interaction for a target within a specific run (for Replay).
    pub(crate) async fn by_run_and_target(
        &self,
        run_id: Uuid,
        target: &str,
    ) -> Result<Option<StoredInteraction>> {
        let row = sqlx::query_as::<_, StoredInteraction>(
            r#"
            SELECT * FROM web_interactions
            WHERE run_id = $1 AND target = $2
            ORDER BY fetched_at DESC
            LIMIT 1
            "#,
        )
        .bind(run_id)
        .bind(target)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row)
    }

    /// Search social topic interactions within a run (for Replay of search_social).
    pub(crate) async fn social_topics_by_run(
        &self,
        run_id: Uuid,
        platform: &str,
    ) -> Result<Option<StoredInteraction>> {
        let row = sqlx::query_as::<_, StoredInteraction>(
            r#"
            SELECT * FROM web_interactions
            WHERE run_id = $1
              AND kind = 'social'
              AND metadata->>'platform' = $2
              AND metadata->>'search_type' = 'topics'
            ORDER BY fetched_at DESC
            LIMIT 1
            "#,
        )
        .bind(run_id)
        .bind(platform)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row)
    }

    /// Find cached semantics by content hash (dedup lookup).
    pub(crate) async fn semantics_by_content_hash(
        &self,
        content_hash: &str,
    ) -> Result<Option<serde_json::Value>> {
        let row = sqlx::query_scalar::<_, serde_json::Value>(
            r#"
            SELECT semantics FROM web_interactions
            WHERE content_hash = $1 AND semantics IS NOT NULL
            ORDER BY fetched_at DESC
            LIMIT 1
            "#,
        )
        .bind(content_hash)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row)
    }

    /// Update semantics on the most recent row matching a content hash.
    pub(crate) async fn update_semantics(
        &self,
        content_hash: &str,
        semantics: &serde_json::Value,
    ) {
        let result = sqlx::query(
            r#"
            UPDATE web_interactions
            SET semantics = $1
            WHERE id = (
                SELECT id FROM web_interactions
                WHERE content_hash = $2
                ORDER BY fetched_at DESC
                LIMIT 1
            )
            "#,
        )
        .bind(semantics)
        .bind(content_hash)
        .execute(&self.pool)
        .await;

        if let Err(e) = result {
            warn!(content_hash, error = %e, "Failed to update semantics");
        }
    }
}
