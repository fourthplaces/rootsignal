use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Listing {
    pub id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub entity_id: Option<Uuid>,
    pub service_id: Option<Uuid>,
    pub source_url: Option<String>,
    pub location_text: Option<String>,
    pub latitude: Option<f32>,
    pub longitude: Option<f32>,
    pub timing_start: Option<DateTime<Utc>>,
    pub timing_end: Option<DateTime<Utc>>,
    pub source_locale: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Listing {
    pub async fn find_by_id(id: Uuid, pool: &PgPool) -> Result<Self> {
        sqlx::query_as::<_, Self>("SELECT * FROM listings WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .map_err(Into::into)
    }

    pub async fn find_active(limit: i64, offset: i64, pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM listings
            WHERE status = 'active'
            ORDER BY created_at DESC
            LIMIT $1 OFFSET $2
            "#,
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_recent(days: i32, pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM listings
            WHERE status = 'active'
              AND timing_start > NOW() - ($1 || ' days')::INTERVAL
            ORDER BY timing_start ASC
            "#,
        )
        .bind(days)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn random_sample(count: i64, pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM listings
            WHERE status = 'active'
            ORDER BY RANDOM()
            LIMIT $1
            "#,
        )
        .bind(count)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn count_active(pool: &PgPool) -> Result<i64> {
        let row = sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*) FROM listings WHERE status = 'active'",
        )
        .fetch_one(pool)
        .await?;
        Ok(row.0)
    }
}

/// Extended listing view with entity name, tags, and provenance.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ListingDetail {
    pub id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub entity_name: Option<String>,
    pub entity_type: Option<String>,
    pub source_url: Option<String>,
    pub location_text: Option<String>,
    pub timing_start: Option<DateTime<Utc>>,
    pub timing_end: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub source_locale: String,
    pub locale: String,
    pub is_fallback: bool,
}

impl ListingDetail {
    /// Find active listings with default English locale (no translation joins).
    pub async fn find_active(limit: i64, offset: i64, pool: &PgPool) -> Result<Vec<Self>> {
        Self::find_active_localized(limit, offset, "en", pool).await
    }

    /// Find active listings with translated content for the given locale.
    /// Fallback chain: requested locale → English → source text.
    pub async fn find_active_localized(
        limit: i64,
        offset: i64,
        locale: &str,
        pool: &PgPool,
    ) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            r#"
            SELECT
                l.id,
                COALESCE(t_title.content, en_title.content, l.title) as title,
                COALESCE(t_desc.content, en_desc.content, l.description) as description,
                l.status,
                e.name as entity_name, e.entity_type,
                l.source_url, l.location_text,
                l.timing_start, l.timing_end, l.created_at,
                l.source_locale,
                CASE
                    WHEN t_title.content IS NOT NULL THEN $1
                    WHEN en_title.content IS NOT NULL THEN 'en'
                    ELSE l.source_locale
                END as locale,
                CASE
                    WHEN t_title.content IS NOT NULL THEN false
                    ELSE true
                END as is_fallback
            FROM listings l
            LEFT JOIN entities e ON e.id = l.entity_id
            LEFT JOIN translations t_title
                ON t_title.translatable_type = 'listing'
                AND t_title.translatable_id = l.id
                AND t_title.field_name = 'title'
                AND t_title.locale = $1
            LEFT JOIN translations t_desc
                ON t_desc.translatable_type = 'listing'
                AND t_desc.translatable_id = l.id
                AND t_desc.field_name = 'description'
                AND t_desc.locale = $1
            LEFT JOIN translations en_title
                ON en_title.translatable_type = 'listing'
                AND en_title.translatable_id = l.id
                AND en_title.field_name = 'title'
                AND en_title.locale = 'en'
            LEFT JOIN translations en_desc
                ON en_desc.translatable_type = 'listing'
                AND en_desc.translatable_id = l.id
                AND en_desc.field_name = 'description'
                AND en_desc.locale = 'en'
            WHERE l.status = 'active'
            ORDER BY l.created_at DESC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(locale)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }
}

/// Stats for the assessment view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListingStats {
    pub total_listings: i64,
    pub active_listings: i64,
    pub total_sources: i64,
    pub total_snapshots: i64,
    pub total_extractions: i64,
    pub total_entities: i64,
    pub listings_by_type: Vec<TagCount>,
    pub listings_by_role: Vec<TagCount>,
    pub listings_by_category: Vec<TagCount>,
    pub recent_7d: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TagCount {
    pub value: String,
    pub count: i64,
}

impl ListingStats {
    pub async fn compute(pool: &PgPool) -> Result<Self> {
        let total_listings = sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM listings")
            .fetch_one(pool)
            .await?
            .0;

        let active_listings =
            sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM listings WHERE status = 'active'")
                .fetch_one(pool)
                .await?
                .0;

        let total_sources = sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM sources")
            .fetch_one(pool)
            .await?
            .0;

        let total_snapshots = sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM page_snapshots")
            .fetch_one(pool)
            .await?
            .0;

        let total_extractions = sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM extractions")
            .fetch_one(pool)
            .await?
            .0;

        let total_entities = sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM entities")
            .fetch_one(pool)
            .await?
            .0;

        let recent_7d = sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*) FROM listings WHERE timing_start > NOW() - INTERVAL '7 days'",
        )
        .fetch_one(pool)
        .await?
        .0;

        let listings_by_type = sqlx::query_as::<_, TagCount>(
            r#"
            SELECT t.value, COUNT(*) as count
            FROM taggables tb
            JOIN tags t ON t.id = tb.tag_id
            WHERE t.kind = 'listing_type' AND tb.taggable_type = 'listing'
            GROUP BY t.value
            ORDER BY count DESC
            "#,
        )
        .fetch_all(pool)
        .await?;

        let listings_by_role = sqlx::query_as::<_, TagCount>(
            r#"
            SELECT t.value, COUNT(*) as count
            FROM taggables tb
            JOIN tags t ON t.id = tb.tag_id
            WHERE t.kind = 'audience_role' AND tb.taggable_type = 'listing'
            GROUP BY t.value
            ORDER BY count DESC
            "#,
        )
        .fetch_all(pool)
        .await?;

        let listings_by_category = sqlx::query_as::<_, TagCount>(
            r#"
            SELECT t.value, COUNT(*) as count
            FROM taggables tb
            JOIN tags t ON t.id = tb.tag_id
            WHERE t.kind = 'category' AND tb.taggable_type = 'listing'
            GROUP BY t.value
            ORDER BY count DESC
            "#,
        )
        .fetch_all(pool)
        .await?;

        Ok(Self {
            total_listings,
            active_listings,
            total_sources,
            total_snapshots,
            total_extractions,
            total_entities,
            listings_by_type,
            listings_by_role,
            listings_by_category,
            recent_7d,
        })
    }
}
