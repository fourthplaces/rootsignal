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
    pub async fn compute_and_store(pool: &PgPool) -> Result<usize> {
        let mut tx = pool.begin().await?;

        sqlx::query("TRUNCATE heat_map_points")
            .execute(&mut *tx)
            .await?;

        // Weight: urgent notes = 10, notice/warning = 5, info/default = 1
        let result = sqlx::query(
            r#"
            INSERT INTO heat_map_points (latitude, longitude, weight, entity_type, entity_id)
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
                la.locatable_id as entity_id
            FROM locationables la
            JOIN locations loc ON loc.id = la.location_id
            LEFT JOIN noteables na ON na.notable_type = la.locatable_type AND na.notable_id = la.locatable_id
            LEFT JOIN notes n ON n.id = na.note_id
            WHERE loc.latitude IS NOT NULL AND loc.longitude IS NOT NULL
            GROUP BY loc.latitude, loc.longitude, la.locatable_type, la.locatable_id
            "#,
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(result.rows_affected() as usize)
    }

    /// Find heat map points near a zip code within a radius.
    pub async fn find_near_zip(
        zip: &str,
        radius_miles: f64,
        pool: &PgPool,
    ) -> Result<Vec<Self>> {
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
