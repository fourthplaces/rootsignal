use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Cluster {
    pub id: Uuid,
    pub cluster_type: String,
    pub representative_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Cluster {
    /// Create a new cluster and add the representative as its first member.
    pub async fn create(
        cluster_type: &str,
        representative_id: Uuid,
        pool: &PgPool,
    ) -> Result<Self> {
        let cluster = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO clusters (cluster_type, representative_id)
            VALUES ($1, $2)
            RETURNING *
            "#,
        )
        .bind(cluster_type)
        .bind(representative_id)
        .fetch_one(pool)
        .await?;

        // Representative must be a member of its own cluster
        super::cluster_item::ClusterItem::create(
            cluster.id,
            representative_id,
            cluster_type,
            None, // representative has no similarity_score to itself
            pool,
        )
        .await?;

        Ok(cluster)
    }

    pub async fn find_by_id(id: Uuid, pool: &PgPool) -> Result<Option<Self>> {
        sqlx::query_as::<_, Self>("SELECT * FROM clusters WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(Into::into)
    }

    pub async fn update_representative(
        cluster_id: Uuid,
        new_representative_id: Uuid,
        pool: &PgPool,
    ) -> Result<()> {
        sqlx::query("UPDATE clusters SET representative_id = $1, updated_at = now() WHERE id = $2")
            .bind(new_representative_id)
            .bind(cluster_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Merge absorb_cluster into keep_cluster: reassign all items, delete absorbed cluster.
    pub async fn merge(keep_id: Uuid, absorb_id: Uuid, pool: &PgPool) -> Result<()> {
        // Reassign all items from absorbed cluster to kept cluster
        // ON CONFLICT handles the case where an item is already in the kept cluster
        sqlx::query(
            r#"
            UPDATE cluster_items SET cluster_id = $1
            WHERE cluster_id = $2
            AND NOT EXISTS (
                SELECT 1 FROM cluster_items ci2
                WHERE ci2.cluster_id = $1
                AND ci2.item_type = cluster_items.item_type
                AND ci2.item_id = cluster_items.item_id
            )
            "#,
        )
        .bind(keep_id)
        .bind(absorb_id)
        .execute(pool)
        .await?;

        // Delete any remaining items in absorbed cluster (duplicates that couldn't move)
        sqlx::query("DELETE FROM cluster_items WHERE cluster_id = $1")
            .bind(absorb_id)
            .execute(pool)
            .await?;

        // Delete the absorbed cluster
        sqlx::query("DELETE FROM clusters WHERE id = $1")
            .bind(absorb_id)
            .execute(pool)
            .await?;

        Ok(())
    }

    /// Count clusters by type.
    pub async fn count_by_type(cluster_type: &str, pool: &PgPool) -> Result<i64> {
        let row =
            sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM clusters WHERE cluster_type = $1")
                .bind(cluster_type)
                .fetch_one(pool)
                .await?;
        Ok(row.0)
    }
}
