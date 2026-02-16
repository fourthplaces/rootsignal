use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

/// Lightweight cluster representation for map markers.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct MapCluster {
    pub id: Uuid,
    pub latitude: f64,
    pub longitude: f64,
    pub member_count: i64,
    pub dominant_signal_type: String,
    pub representative_content: String,
    pub representative_about: Option<String>,
    pub ask_count: i64,
    pub give_count: i64,
    pub event_count: i64,
    pub informative_count: i64,
    pub entity_names: serde_json::Value,
}

/// Full cluster detail for sidebar display.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ClusterDetail {
    pub id: Uuid,
    pub cluster_type: String,
    pub representative_id: Uuid,
    pub representative_content: String,
    pub representative_about: Option<String>,
    pub representative_signal_type: String,
    pub representative_confidence: f64,
    pub representative_broadcasted_at: Option<DateTime<Utc>>,
}

/// A signal belonging to a cluster, for sidebar listing.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ClusterSignal {
    pub id: Uuid,
    pub signal_type: String,
    pub content: String,
    pub confidence: f64,
    pub broadcasted_at: Option<DateTime<Utc>>,
}

/// An entity linked to signals in a cluster.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ClusterEntity {
    pub id: Uuid,
    pub name: String,
    pub entity_type: String,
}

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
        // Also update cluster_type to match the kept cluster
        sqlx::query(
            r#"
            UPDATE cluster_items SET cluster_id = $1,
                cluster_type = (SELECT cluster_type FROM clusters WHERE id = $1)
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

    /// Fetch clusters with location data for map display.
    /// Filters apply to the representative signal. Only returns clusters with 2+ members
    /// and a geocoded representative.
    pub async fn find_for_map(
        signal_type: Option<&str>,
        since: Option<&str>,
        min_confidence: Option<f64>,
        zip_code: Option<&str>,
        radius_miles: Option<f64>,
        about: Option<&str>,
        limit: i64,
        pool: &PgPool,
    ) -> Result<Vec<MapCluster>> {
        let mut qb = sqlx::QueryBuilder::new(
            r#"
            WITH cluster_counts AS (
                SELECT
                    ci.cluster_id,
                    COUNT(*) AS member_count,
                    COUNT(*) FILTER (WHERE s.signal_type = 'ask') AS ask_count,
                    COUNT(*) FILTER (WHERE s.signal_type = 'give') AS give_count,
                    COUNT(*) FILTER (WHERE s.signal_type = 'event') AS event_count,
                    COUNT(*) FILTER (WHERE s.signal_type = 'informative') AS informative_count,
                    COALESCE(
                        jsonb_agg(DISTINCT e.name) FILTER (WHERE e.name IS NOT NULL),
                        '[]'::jsonb
                    ) AS entity_names
                FROM cluster_items ci
                JOIN signals s ON s.id = ci.item_id AND ci.item_type = 'signal'
                LEFT JOIN entities e ON e.id = s.entity_id
                WHERE ci.cluster_type = 'signal'
                GROUP BY ci.cluster_id
                HAVING COUNT(*) >= 2
            )
            SELECT
                c.id,
                loc.latitude,
                loc.longitude,
                cc.member_count,
                CASE
                    WHEN cc.ask_count >= cc.give_count
                        AND cc.ask_count >= cc.event_count
                        AND cc.ask_count >= cc.informative_count THEN 'ask'
                    WHEN cc.give_count >= cc.event_count
                        AND cc.give_count >= cc.informative_count THEN 'give'
                    WHEN cc.event_count >= cc.informative_count THEN 'event'
                    ELSE 'informative'
                END AS dominant_signal_type,
                LEFT(rep.content, 200) AS representative_content,
                rep.about AS representative_about,
                cc.ask_count,
                cc.give_count,
                cc.event_count,
                cc.informative_count,
                cc.entity_names
            FROM clusters c
            JOIN cluster_counts cc ON cc.cluster_id = c.id
            JOIN signals rep ON rep.id = c.representative_id
            JOIN locationables la ON la.locatable_type = 'signal' AND la.locatable_id = rep.id
            JOIN locations loc ON loc.id = la.location_id
                AND loc.latitude IS NOT NULL AND loc.longitude IS NOT NULL
            WHERE c.cluster_type = 'signal'
            "#,
        );

        if let Some(st) = signal_type {
            qb.push("AND rep.signal_type = ");
            qb.push_bind(st);
            qb.push(" ");
        }

        if let Some(since_period) = since {
            let interval = match since_period {
                "24h" => "1 day",
                "week" => "7 days",
                "month" => "30 days",
                _ => "30 days",
            };
            qb.push("AND COALESCE(rep.broadcasted_at, rep.created_at) >= now() - interval '");
            qb.push(interval);
            qb.push("' ");
        }

        if let Some(conf) = min_confidence {
            qb.push("AND rep.confidence >= ");
            qb.push_bind(conf);
            qb.push(" ");
        }

        if let Some(about_term) = about {
            qb.push("AND rep.about ILIKE ");
            qb.push_bind(format!("%{about_term}%"));
            qb.push(" ");
        }

        if let Some(zip) = zip_code {
            let radius = radius_miles.unwrap_or(25.0).min(100.0);
            let lat_delta = radius / 69.0;
            let lng_delta = radius / 54.6;
            qb.push(
                "AND EXISTS (
                    SELECT 1 FROM zip_codes zc
                    WHERE zc.zip_code = ",
            );
            qb.push_bind(zip);
            qb.push(
                " AND loc.latitude BETWEEN zc.latitude - ",
            );
            qb.push_bind(lat_delta);
            qb.push(" AND zc.latitude + ");
            qb.push_bind(lat_delta);
            qb.push(" AND loc.longitude BETWEEN zc.longitude - ");
            qb.push_bind(lng_delta);
            qb.push(" AND zc.longitude + ");
            qb.push_bind(lng_delta);
            qb.push(" AND haversine_distance(zc.latitude, zc.longitude, loc.latitude, loc.longitude) <= ");
            qb.push_bind(radius);
            qb.push(") ");
        }

        qb.push("ORDER BY cc.member_count DESC LIMIT ");
        qb.push_bind(limit);

        qb.build_query_as::<MapCluster>()
            .fetch_all(pool)
            .await
            .map_err(Into::into)
    }

    /// Fetch a single cluster's detail for the sidebar.
    pub async fn find_detail(id: Uuid, pool: &PgPool) -> Result<Option<ClusterDetail>> {
        sqlx::query_as::<_, ClusterDetail>(
            r#"
            SELECT
                c.id,
                c.cluster_type,
                c.representative_id,
                LEFT(rep.content, 200) AS representative_content,
                rep.about AS representative_about,
                rep.signal_type AS representative_signal_type,
                rep.confidence AS representative_confidence,
                rep.broadcasted_at AS representative_broadcasted_at
            FROM clusters c
            JOIN signals rep ON rep.id = c.representative_id
            WHERE c.id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(Into::into)
    }

    /// Fetch signals belonging to a cluster.
    pub async fn find_signals(cluster_id: Uuid, limit: i64, pool: &PgPool) -> Result<Vec<ClusterSignal>> {
        sqlx::query_as::<_, ClusterSignal>(
            r#"
            SELECT
                s.id,
                s.signal_type,
                LEFT(s.content, 200) AS content,
                s.confidence,
                s.broadcasted_at
            FROM cluster_items ci
            JOIN signals s ON s.id = ci.item_id AND ci.item_type = 'signal'
            WHERE ci.cluster_id = $1
            ORDER BY COALESCE(s.broadcasted_at, s.created_at) DESC
            LIMIT $2
            "#,
        )
        .bind(cluster_id)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    /// Fetch distinct entities linked to signals in a cluster.
    pub async fn find_entities(cluster_id: Uuid, pool: &PgPool) -> Result<Vec<ClusterEntity>> {
        sqlx::query_as::<_, ClusterEntity>(
            r#"
            SELECT DISTINCT ON (e.id)
                e.id,
                e.name,
                e.entity_type
            FROM cluster_items ci
            JOIN signals s ON s.id = ci.item_id AND ci.item_type = 'signal'
            JOIN entities e ON e.id = s.entity_id
            WHERE ci.cluster_id = $1
            ORDER BY e.id
            "#,
        )
        .bind(cluster_id)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }
}
