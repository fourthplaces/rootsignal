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
    pub source_citation_url: Option<String>,
    pub confidence: f32,
    pub fingerprint: Option<Vec<u8>>,
    pub schema_version: i32,
    pub in_language: String,
    pub broadcasted_at: Option<DateTime<Utc>>,
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
    /// Insert a new signal (no fingerprint-based dedup).
    pub async fn insert(
        signal_type: &str,
        content: &str,
        about: Option<&str>,
        entity_id: Option<Uuid>,
        source_url: Option<&str>,
        page_snapshot_id: Option<Uuid>,
        extraction_id: Option<Uuid>,
        source_citation_url: Option<&str>,
        confidence: f32,
        in_language: &str,
        broadcasted_at: Option<DateTime<Utc>>,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO signals (
                signal_type, content, about, entity_id, source_url,
                page_snapshot_id, extraction_id, source_citation_url,
                confidence, in_language, broadcasted_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
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
        .bind(source_citation_url)
        .bind(confidence)
        .bind(in_language)
        .bind(broadcasted_at)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    /// Update an existing signal with fresh extraction data.
    pub async fn update_from_extraction(
        id: Uuid,
        signal_type: &str,
        content: &str,
        about: Option<&str>,
        entity_id: Option<Uuid>,
        source_url: Option<&str>,
        page_snapshot_id: Option<Uuid>,
        extraction_id: Option<Uuid>,
        confidence: f32,
        broadcasted_at: Option<DateTime<Utc>>,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            UPDATE signals SET
                signal_type = $2,
                content = $3,
                about = $4,
                entity_id = $5,
                source_url = $6,
                page_snapshot_id = $7,
                extraction_id = $8,
                confidence = $9,
                broadcasted_at = COALESCE($10, signals.broadcasted_at),
                updated_at = NOW()
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(id)
        .bind(signal_type)
        .bind(content)
        .bind(about)
        .bind(entity_id)
        .bind(source_url)
        .bind(page_snapshot_id)
        .bind(extraction_id)
        .bind(confidence)
        .bind(broadcasted_at)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    /// Fetch signals previously extracted from the same URL.
    pub async fn find_by_url(url: &str, limit: i64, pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            r#"
            SELECT s.* FROM signals s
            JOIN page_snapshots ps ON ps.id = s.page_snapshot_id
            WHERE ps.canonical_url = $1 OR ps.url = $1
            ORDER BY s.broadcasted_at DESC NULLS LAST, s.created_at DESC
            LIMIT $2
            "#,
        )
        .bind(url)
        .bind(limit)
        .fetch_all(pool)
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

    pub async fn find_by_source(
        source_id: Uuid,
        limit: i64,
        offset: i64,
        pool: &PgPool,
    ) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            r#"
            SELECT s.* FROM signals s
            JOIN page_snapshots ps ON ps.id = s.page_snapshot_id
            JOIN domain_snapshots ds ON ds.page_snapshot_id = ps.id
            WHERE ds.source_id = $1
            ORDER BY s.created_at DESC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(source_id)
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

    /// Delete all signals associated with a source (via domain_snapshots → page_snapshots).
    /// Also cleans up polymorphic associations (locationables, taggables, schedules).
    pub async fn delete_by_source(source_id: Uuid, pool: &PgPool) -> Result<u64> {
        let signal_ids: Vec<Uuid> = sqlx::query_as::<_, (Uuid,)>(
            r#"
            SELECT s.id FROM signals s
            JOIN page_snapshots ps ON ps.id = s.page_snapshot_id
            JOIN domain_snapshots ds ON ds.page_snapshot_id = ps.id
            WHERE ds.source_id = $1
            "#,
        )
        .bind(source_id)
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|r| r.0)
        .collect();

        if signal_ids.is_empty() {
            return Ok(0);
        }

        // Clean up polymorphic associations
        sqlx::query("DELETE FROM locationables WHERE locatable_type = 'signal' AND locatable_id = ANY($1)")
            .bind(&signal_ids)
            .execute(pool)
            .await?;

        sqlx::query("DELETE FROM taggables WHERE taggable_type = 'signal' AND taggable_id = ANY($1)")
            .bind(&signal_ids)
            .execute(pool)
            .await?;

        sqlx::query("DELETE FROM schedules WHERE scheduleable_type = 'signal' AND scheduleable_id = ANY($1)")
            .bind(&signal_ids)
            .execute(pool)
            .await?;

        sqlx::query("DELETE FROM cluster_items WHERE item_type = 'signal' AND item_id = ANY($1)")
            .bind(&signal_ids)
            .execute(pool)
            .await?;

        // Delete signals (cascades to signal_flags, sets NULL on findings.trigger_signal_id)
        let result = sqlx::query("DELETE FROM signals WHERE id = ANY($1)")
            .bind(&signal_ids)
            .execute(pool)
            .await?;

        Ok(result.rows_affected())
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

/// Tag count result row.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TagCount {
    pub value: String,
    pub count: i64,
}

/// Aggregate statistics for signals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalStats {
    pub total_signals: i64,
    pub total_sources: i64,
    pub total_snapshots: i64,
    pub total_extractions: i64,
    pub total_entities: i64,
    pub signals_by_type: Vec<TagCount>,
    pub signals_by_domain: Vec<TagCount>,
    pub recent_7d: i64,
}

impl SignalStats {
    pub async fn compute(pool: &PgPool) -> Result<Self> {
        let (
            total_signals,
            total_sources,
            total_snapshots,
            total_extractions,
            total_entities,
            recent_7d,
            signals_by_type,
            signals_by_domain,
        ) = tokio::try_join!(
            async {
                sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM signals")
                    .fetch_one(pool)
                    .await
                    .map(|r| r.0)
                    .map_err(anyhow::Error::from)
            },
            async {
                sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM sources")
                    .fetch_one(pool)
                    .await
                    .map(|r| r.0)
                    .map_err(anyhow::Error::from)
            },
            async {
                sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM page_snapshots")
                    .fetch_one(pool)
                    .await
                    .map(|r| r.0)
                    .map_err(anyhow::Error::from)
            },
            async {
                sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM extractions")
                    .fetch_one(pool)
                    .await
                    .map(|r| r.0)
                    .map_err(anyhow::Error::from)
            },
            async {
                sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM entities")
                    .fetch_one(pool)
                    .await
                    .map(|r| r.0)
                    .map_err(anyhow::Error::from)
            },
            async {
                sqlx::query_as::<_, (i64,)>(
                    "SELECT COUNT(*) FROM signals WHERE created_at > NOW() - INTERVAL '7 days'",
                )
                .fetch_one(pool)
                .await
                .map(|r| r.0)
                .map_err(anyhow::Error::from)
            },
            Self::count_by_signal_type(pool),
            Self::count_by_tag_kind("signal_domain", pool),
        )?;

        Ok(Self {
            total_signals,
            total_sources,
            total_snapshots,
            total_extractions,
            total_entities,
            signals_by_type,
            signals_by_domain,
            recent_7d,
        })
    }

    async fn count_by_signal_type(pool: &PgPool) -> Result<Vec<TagCount>> {
        sqlx::query_as::<_, TagCount>(
            "SELECT signal_type AS value, COUNT(*) AS count FROM signals GROUP BY signal_type ORDER BY count DESC",
        )
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    async fn count_by_tag_kind(kind: &str, pool: &PgPool) -> Result<Vec<TagCount>> {
        sqlx::query_as::<_, TagCount>(
            r#"
            SELECT t.value, COUNT(*) as count
            FROM taggables tb
            JOIN tags t ON t.id = tb.tag_id
            WHERE t.kind = $1 AND tb.taggable_type = 'signal'
            GROUP BY t.value
            ORDER BY count DESC
            "#,
        )
        .bind(kind)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }
}
