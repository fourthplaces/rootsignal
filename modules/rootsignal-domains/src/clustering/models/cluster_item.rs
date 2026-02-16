use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ClusterItem {
    pub id: Uuid,
    pub cluster_id: Uuid,
    pub item_id: Uuid,
    pub item_type: String,
    pub cluster_type: String,
    pub similarity_score: Option<f32>,
    pub added_at: DateTime<Utc>,
}

impl ClusterItem {
    /// Create a cluster item with explicit cluster_type for multi-dimension support.
    pub async fn create(
        cluster_id: Uuid,
        item_id: Uuid,
        item_type: &str,
        cluster_type: &str,
        similarity_score: Option<f32>,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO cluster_items (cluster_id, item_id, item_type, cluster_type, similarity_score)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (cluster_type, item_type, item_id) DO UPDATE
                SET cluster_id = EXCLUDED.cluster_id,
                    similarity_score = EXCLUDED.similarity_score,
                    added_at = now()
            RETURNING *
            "#,
        )
        .bind(cluster_id)
        .bind(item_id)
        .bind(item_type)
        .bind(cluster_type)
        .bind(similarity_score)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    /// Find which cluster an item belongs to within a given cluster_type dimension.
    pub async fn find_cluster_for<'e, E: sqlx::Executor<'e, Database = sqlx::Postgres>>(
        item_type: &str,
        item_id: Uuid,
        cluster_type: &str,
        executor: E,
    ) -> Result<Option<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM cluster_items WHERE item_type = $1 AND item_id = $2 AND cluster_type = $3",
        )
        .bind(item_type)
        .bind(item_id)
        .bind(cluster_type)
        .fetch_optional(executor)
        .await
        .map_err(Into::into)
    }

    /// Get all items in a cluster.
    pub async fn items_in_cluster(cluster_id: Uuid, pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM cluster_items WHERE cluster_id = $1 ORDER BY similarity_score DESC NULLS LAST",
        )
        .bind(cluster_id)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    /// Find signals that are not yet in any cluster for the given cluster_type dimension.
    pub async fn unclustered_signals<'e, E: sqlx::Executor<'e, Database = sqlx::Postgres>>(
        cluster_type: &str,
        limit: i64,
        executor: E,
    ) -> Result<Vec<Uuid>> {
        let rows = sqlx::query_as::<_, (Uuid,)>(
            r#"
            SELECT s.id
            FROM signals s
            JOIN embeddings e ON e.embeddable_type = 'signal' AND e.embeddable_id = s.id AND e.locale = 'en'
            WHERE NOT EXISTS (
                SELECT 1 FROM cluster_items ci
                WHERE ci.item_type = 'signal' AND ci.item_id = s.id AND ci.cluster_type = $1
            )
            ORDER BY s.created_at ASC
            LIMIT $2
            "#,
        )
        .bind(cluster_type)
        .bind(limit)
        .fetch_all(executor)
        .await?;

        Ok(rows.into_iter().map(|r| r.0).collect())
    }
}
