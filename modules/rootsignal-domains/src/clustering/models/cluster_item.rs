use anyhow::{bail, Result};
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
    pub similarity_score: Option<f32>,
    pub added_at: DateTime<Utc>,
}

impl ClusterItem {
    /// Create a cluster item, validating that item_type matches the cluster's cluster_type.
    pub async fn create(
        cluster_id: Uuid,
        item_id: Uuid,
        item_type: &str,
        similarity_score: Option<f32>,
        pool: &PgPool,
    ) -> Result<Self> {
        // Validate item_type matches cluster_type
        let cluster_type = sqlx::query_as::<_, (String,)>(
            "SELECT cluster_type FROM clusters WHERE id = $1",
        )
        .bind(cluster_id)
        .fetch_one(pool)
        .await?;

        if cluster_type.0 != item_type {
            bail!(
                "item_type '{}' does not match cluster_type '{}'",
                item_type,
                cluster_type.0
            );
        }

        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO cluster_items (cluster_id, item_id, item_type, similarity_score)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (item_type, item_id) DO UPDATE
                SET cluster_id = EXCLUDED.cluster_id,
                    similarity_score = EXCLUDED.similarity_score,
                    added_at = now()
            RETURNING *
            "#,
        )
        .bind(cluster_id)
        .bind(item_id)
        .bind(item_type)
        .bind(similarity_score)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    /// Find which cluster an item belongs to.
    pub async fn find_cluster_for<'e, E: sqlx::Executor<'e, Database = sqlx::Postgres>>(
        item_type: &str,
        item_id: Uuid,
        executor: E,
    ) -> Result<Option<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM cluster_items WHERE item_type = $1 AND item_id = $2",
        )
        .bind(item_type)
        .bind(item_id)
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

    /// Find items of a given type that are not yet in any cluster.
    pub async fn unclustered<'e, E: sqlx::Executor<'e, Database = sqlx::Postgres>>(
        item_type: &str,
        limit: i64,
        executor: E,
    ) -> Result<Vec<Uuid>> {
        let rows = sqlx::query_as::<_, (Uuid,)>(
            r#"
            SELECT l.id
            FROM listings l
            JOIN embeddings e ON e.embeddable_type = 'listing' AND e.embeddable_id = l.id AND e.locale = 'en'
            WHERE l.id NOT IN (SELECT item_id FROM cluster_items WHERE item_type = $1)
            ORDER BY l.created_at ASC
            LIMIT $2
            "#,
        )
        .bind(item_type)
        .bind(limit)
        .fetch_all(executor)
        .await?;

        Ok(rows.into_iter().map(|r| r.0).collect())
    }
}
