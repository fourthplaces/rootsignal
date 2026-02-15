use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Signal {
    pub id: Uuid,
    pub signal_type: String,
    pub content: String,
    pub about: Option<String>,
    pub entity_id: Option<Uuid>,
    pub source_url: Option<String>,
    pub page_snapshot_id: Option<Uuid>,
    pub extraction_id: Option<Uuid>,
    pub institutional_source: Option<String>,
    pub institutional_record_id: Option<String>,
    pub source_citation_url: Option<String>,
    pub confidence: f32,
    pub fingerprint: Vec<u8>,
    pub schema_version: i32,
    pub in_language: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Signal with Haversine distance (for geo queries via locationables).
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SignalWithDistance {
    #[sqlx(flatten)]
    pub signal: Signal,
    pub distance_km: Option<f64>,
}

impl Signal {
    pub async fn create(
        signal_type: &str,
        content: &str,
        about: Option<&str>,
        entity_id: Option<Uuid>,
        source_url: Option<&str>,
        page_snapshot_id: Option<Uuid>,
        extraction_id: Option<Uuid>,
        institutional_source: Option<&str>,
        institutional_record_id: Option<&str>,
        source_citation_url: Option<&str>,
        confidence: f32,
        fingerprint: &[u8],
        in_language: &str,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO signals (
                signal_type, content, about, entity_id, source_url,
                page_snapshot_id, extraction_id,
                institutional_source, institutional_record_id, source_citation_url,
                confidence, fingerprint, in_language
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
            ON CONFLICT (fingerprint, schema_version) DO UPDATE SET
                content = EXCLUDED.content,
                about = EXCLUDED.about,
                entity_id = EXCLUDED.entity_id,
                confidence = EXCLUDED.confidence,
                updated_at = NOW()
            RETURNING *
            "#,
        )
        .bind(signal_type)
        .bind(content)
        .bind(about)
        .bind(entity_id)
        .bind(source_url)
        .bind(page_snapshot_id)
        .bind(extraction_id)
        .bind(institutional_source)
        .bind(institutional_record_id)
        .bind(source_citation_url)
        .bind(confidence)
        .bind(fingerprint)
        .bind(in_language)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_by_id(id: Uuid, pool: &PgPool) -> Result<Self> {
        sqlx::query_as::<_, Self>("SELECT * FROM signals WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .map_err(Into::into)
    }

    pub async fn find_by_type(
        signal_type: &str,
        limit: i64,
        offset: i64,
        pool: &PgPool,
    ) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM signals WHERE signal_type = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3",
        )
        .bind(signal_type)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_by_entity(
        entity_id: Uuid,
        limit: i64,
        offset: i64,
        pool: &PgPool,
    ) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM signals WHERE entity_id = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3",
        )
        .bind(entity_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_all(limit: i64, offset: i64, pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM signals ORDER BY created_at DESC LIMIT $1 OFFSET $2",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    /// Full-text search on content + about.
    pub async fn search(
        query: &str,
        signal_type: Option<&str>,
        limit: i64,
        offset: i64,
        pool: &PgPool,
    ) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM signals
            WHERE search_vector @@ plainto_tsquery('english', $1)
              AND ($2::text IS NULL OR signal_type = $2)
            ORDER BY ts_rank(search_vector, plainto_tsquery('english', $1)) DESC
            LIMIT $3 OFFSET $4
            "#,
        )
        .bind(query)
        .bind(signal_type)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    /// Geo search: find signals near a lat/lng via locationables join (Haversine).
    pub async fn find_near(
        lat: f64,
        lng: f64,
        radius_km: f64,
        signal_type: Option<&str>,
        limit: i64,
        pool: &PgPool,
    ) -> Result<Vec<SignalWithDistance>> {
        sqlx::query_as::<_, SignalWithDistance>(
            r#"
            SELECT s.*,
                (6371 * acos(
                    cos(radians($1)) * cos(radians(l.latitude)) *
                    cos(radians(l.longitude) - radians($2)) +
                    sin(radians($1)) * sin(radians(l.latitude))
                )) AS distance_km
            FROM signals s
            JOIN locationables la ON la.locatable_type = 'signal' AND la.locatable_id = s.id
            JOIN locations l ON l.id = la.location_id
            WHERE l.latitude IS NOT NULL AND l.longitude IS NOT NULL
              AND ($4::text IS NULL OR s.signal_type = $4)
            HAVING (6371 * acos(
                cos(radians($1)) * cos(radians(l.latitude)) *
                cos(radians(l.longitude) - radians($2)) +
                sin(radians($1)) * sin(radians(l.latitude))
            )) < $3
            ORDER BY distance_km ASC
            LIMIT $5
            "#,
        )
        .bind(lat)
        .bind(lng)
        .bind(radius_km)
        .bind(signal_type)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn count(pool: &PgPool) -> Result<i64> {
        let row = sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM signals")
            .fetch_one(pool)
            .await?;
        Ok(row.0)
    }

    pub async fn count_by_type(signal_type: &str, pool: &PgPool) -> Result<i64> {
        let row =
            sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM signals WHERE signal_type = $1")
                .bind(signal_type)
                .fetch_one(pool)
                .await?;
        Ok(row.0)
    }
}
