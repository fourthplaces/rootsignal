use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct HeatMapPoint {
    pub id: Uuid,
    pub latitude: f64,
    pub longitude: f64,
    pub weight: f64,
    pub entity_type: String,
    pub entity_id: Uuid,
    pub generated_at: DateTime<Utc>,
}

/// Signal density aggregated by zip code.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ZipDensity {
    pub zip_code: String,
    pub address_locality: String,
    pub latitude: f64,
    pub longitude: f64,
    pub listing_count: i64,
    pub signal_domain_counts: serde_json::Value,
}

/// Temporal comparison between two periods.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TemporalDelta {
    pub zip_code: String,
    pub latitude: f64,
    pub longitude: f64,
    pub current_count: i64,
    pub previous_count: i64,
    pub delta: i64,
    pub change_pct: f64,
}

impl HeatMapPoint {
    pub async fn find_latest(pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM heat_map_points
            WHERE generated_at = (SELECT MAX(generated_at) FROM heat_map_points)
            "#,
        )
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_latest_by_type(entity_type: &str, pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM heat_map_points
            WHERE entity_type = $1
              AND generated_at = (
                  SELECT MAX(generated_at) FROM heat_map_points WHERE entity_type = $1
              )
            "#,
        )
        .bind(entity_type)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    /// Recompute heat map points from locationables + notes urgency weighting.
    /// Truncates existing points and inserts fresh ones in a transaction.
    /// Enriches listing points with signal_domain, category, and listing_type from tags.
    pub async fn compute_and_store(pool: &PgPool) -> Result<usize> {
        let mut tx = pool.begin().await?;

        sqlx::query("TRUNCATE heat_map_points")
            .execute(&mut *tx)
            .await?;

        // Weight: urgent notes = 10, notice/warning = 5, info/default = 1
        // Enrich with tag metadata for listings
        let result = sqlx::query(
            r#"
            INSERT INTO heat_map_points (latitude, longitude, weight, entity_type, entity_id, signal_domain, category, listing_type)
            SELECT
                loc.latitude,
                loc.longitude,
                COALESCE(MAX(
                    CASE n.severity
                        WHEN 'critical' THEN 10.0
                        WHEN 'warning' THEN 5.0
                        ELSE 1.0
                    END
                ), 1.0) as weight,
                la.locatable_type as entity_type,
                la.locatable_id as entity_id,
                MAX(CASE WHEN t.kind = 'signal_domain' THEN t.value END) as signal_domain,
                MAX(CASE WHEN t.kind = 'category' THEN t.value END) as category,
                MAX(CASE WHEN t.kind = 'listing_type' THEN t.value END) as listing_type
            FROM locationables la
            JOIN locations loc ON loc.id = la.location_id
            LEFT JOIN noteables na ON na.notable_type = la.locatable_type AND na.notable_id = la.locatable_id
            LEFT JOIN notes n ON n.id = na.note_id
            LEFT JOIN taggables tg ON tg.taggable_type = la.locatable_type AND tg.taggable_id = la.locatable_id
            LEFT JOIN tags t ON t.id = tg.tag_id AND t.kind IN ('signal_domain', 'category', 'listing_type')
            WHERE loc.latitude IS NOT NULL AND loc.longitude IS NOT NULL
            GROUP BY loc.latitude, loc.longitude, la.locatable_type, la.locatable_id
            "#,
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(result.rows_affected() as usize)
    }

    /// Find heat map points by signal domain.
    pub async fn find_by_domain(signal_domain: &str, pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            r#"
            SELECT id, latitude, longitude, weight, entity_type, entity_id, generated_at
            FROM heat_map_points
            WHERE signal_domain = $1
              AND generated_at = (SELECT MAX(generated_at) FROM heat_map_points)
            "#,
        )
        .bind(signal_domain)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    /// Signal density: listing counts per zip code with domain breakdown.
    pub async fn signal_density_by_zip(
        signal_domain: Option<&str>,
        category: Option<&str>,
        pool: &PgPool,
    ) -> Result<Vec<ZipDensity>> {
        let mut qb = sqlx::QueryBuilder::new(
            r#"SELECT
                z.zip_code,
                z.address_locality,
                z.latitude,
                z.longitude,
                COUNT(DISTINCT h.entity_id) AS listing_count,
                COALESCE(
                    jsonb_object_agg(
                        COALESCE(h.signal_domain, 'unknown'),
                        domain_counts.cnt
                    ) FILTER (WHERE domain_counts.cnt IS NOT NULL),
                    '{}'::jsonb
                ) AS signal_domain_counts
            FROM zip_codes z
            LEFT JOIN heat_map_points h ON
                haversine_distance(z.latitude, z.longitude, h.latitude, h.longitude) <= 5.0
                AND h.generated_at = (SELECT MAX(generated_at) FROM heat_map_points)
            LEFT JOIN LATERAL (
                SELECT h.signal_domain, COUNT(*) AS cnt
                FROM heat_map_points h2
                WHERE haversine_distance(z.latitude, z.longitude, h2.latitude, h2.longitude) <= 5.0
                  AND h2.generated_at = h.generated_at
                  AND h2.signal_domain = h.signal_domain
                GROUP BY h.signal_domain
            ) domain_counts ON true
            WHERE 1=1 "#,
        );

        if let Some(sd) = signal_domain {
            qb.push("AND h.signal_domain = ");
            qb.push_bind(sd);
            qb.push(" ");
        }
        if let Some(cat) = category {
            qb.push("AND h.category = ");
            qb.push_bind(cat);
            qb.push(" ");
        }

        qb.push("GROUP BY z.zip_code, z.address_locality, z.latitude, z.longitude ORDER BY listing_count DESC");

        qb.build_query_as::<ZipDensity>()
            .fetch_all(pool)
            .await
            .map_err(Into::into)
    }

    /// Signal gaps: zip codes with lowest signal coverage.
    pub async fn signal_gaps(
        signal_domain: Option<&str>,
        category: Option<&str>,
        limit: i64,
        pool: &PgPool,
    ) -> Result<Vec<ZipDensity>> {
        let mut qb = sqlx::QueryBuilder::new(
            r#"SELECT
                z.zip_code,
                z.address_locality,
                z.latitude,
                z.longitude,
                COUNT(DISTINCT h.entity_id) AS listing_count,
                '{}'::jsonb AS signal_domain_counts
            FROM zip_codes z
            LEFT JOIN heat_map_points h ON
                haversine_distance(z.latitude, z.longitude, h.latitude, h.longitude) <= 5.0
                AND h.generated_at = (SELECT MAX(generated_at) FROM heat_map_points) "#,
        );

        if let Some(sd) = signal_domain {
            qb.push("AND h.signal_domain = ");
            qb.push_bind(sd);
            qb.push(" ");
        }
        if let Some(cat) = category {
            qb.push("AND h.category = ");
            qb.push_bind(cat);
            qb.push(" ");
        }

        qb.push("GROUP BY z.zip_code, z.address_locality, z.latitude, z.longitude ORDER BY listing_count ASC LIMIT ");
        qb.push_bind(limit);

        qb.build_query_as::<ZipDensity>()
            .fetch_all(pool)
            .await
            .map_err(Into::into)
    }

    /// Temporal comparison: current period vs previous period.
    pub async fn temporal_comparison(
        current_start: DateTime<Utc>,
        current_end: DateTime<Utc>,
        previous_start: DateTime<Utc>,
        previous_end: DateTime<Utc>,
        signal_domain: Option<&str>,
        pool: &PgPool,
    ) -> Result<Vec<TemporalDelta>> {
        let mut qb = sqlx::QueryBuilder::new(
            r#"WITH current_period AS (
                SELECT z.zip_code, z.latitude, z.longitude, COUNT(DISTINCT h.entity_id) AS cnt
                FROM zip_codes z
                LEFT JOIN heat_map_points h ON
                    haversine_distance(z.latitude, z.longitude, h.latitude, h.longitude) <= 5.0
                    AND h.generated_at BETWEEN "#,
        );
        qb.push_bind(current_start);
        qb.push(" AND ");
        qb.push_bind(current_end);
        if let Some(sd) = signal_domain {
            qb.push(" AND h.signal_domain = ");
            qb.push_bind(sd);
        }
        qb.push(
            " GROUP BY z.zip_code, z.latitude, z.longitude \
            ), previous_period AS ( \
                SELECT z.zip_code, COUNT(DISTINCT h.entity_id) AS cnt \
                FROM zip_codes z \
                LEFT JOIN heat_map_points h ON \
                    haversine_distance(z.latitude, z.longitude, h.latitude, h.longitude) <= 5.0 \
                    AND h.generated_at BETWEEN ",
        );
        qb.push_bind(previous_start);
        qb.push(" AND ");
        qb.push_bind(previous_end);
        if let Some(sd) = signal_domain {
            qb.push(" AND h.signal_domain = ");
            qb.push_bind(sd);
        }
        qb.push(
            " GROUP BY z.zip_code \
            ) SELECT \
                c.zip_code, c.latitude, c.longitude, \
                c.cnt AS current_count, \
                COALESCE(p.cnt, 0) AS previous_count, \
                (c.cnt - COALESCE(p.cnt, 0)) AS delta, \
                CASE WHEN COALESCE(p.cnt, 0) = 0 THEN 0.0 \
                     ELSE ((c.cnt - p.cnt)::double precision / p.cnt::double precision * 100.0) \
                END AS change_pct \
            FROM current_period c \
            LEFT JOIN previous_period p ON p.zip_code = c.zip_code \
            ORDER BY delta DESC",
        );

        qb.build_query_as::<TemporalDelta>()
            .fetch_all(pool)
            .await
            .map_err(Into::into)
    }

    /// Find heat map points near a zip code within a radius.
    pub async fn find_near_zip(zip: &str, radius_miles: f64, pool: &PgPool) -> Result<Vec<Self>> {
        let radius_miles = radius_miles.min(100.0);
        let lat_delta = radius_miles / 69.0;
        let lng_delta = radius_miles / 54.6;

        sqlx::query_as::<_, Self>(
            r#"
            WITH center AS (
                SELECT latitude, longitude FROM zip_codes WHERE zip_code = $1
            )
            SELECT h.*
            FROM heat_map_points h
            CROSS JOIN center
            WHERE h.latitude BETWEEN center.latitude - $2 AND center.latitude + $2
              AND h.longitude BETWEEN center.longitude - $3 AND center.longitude + $3
              AND haversine_distance(center.latitude, center.longitude, h.latitude, h.longitude) <= $4
              AND h.generated_at = (SELECT MAX(generated_at) FROM heat_map_points)
            "#,
        )
        .bind(zip)
        .bind(lat_delta)
        .bind(lng_delta)
        .bind(radius_miles)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }
}
