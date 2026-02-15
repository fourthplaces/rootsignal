use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Source {
    pub id: Uuid,
    pub entity_id: Option<Uuid>,
    pub name: String,
    pub source_type: String,
    pub url: Option<String>,
    pub handle: Option<String>,
    pub cadence_hours: i32,
    pub last_scraped_at: Option<DateTime<Utc>>,
    pub is_active: bool,
    pub config: serde_json::Value,
    pub qualification_status: String,
    pub qualification_summary: Option<String>,
    pub qualification_score: Option<i32>,
    pub created_at: DateTime<Utc>,
}

impl Source {
    pub async fn create(
        name: &str,
        source_type: &str,
        url: Option<&str>,
        handle: Option<&str>,
        entity_id: Option<Uuid>,
        cadence_hours: Option<i32>,
        config: serde_json::Value,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO sources (name, source_type, url, handle, entity_id, cadence_hours, config)
            VALUES ($1, $2, $3, $4, $5, COALESCE($6, 24), $7)
            RETURNING *
            "#,
        )
        .bind(name)
        .bind(source_type)
        .bind(url)
        .bind(handle)
        .bind(entity_id)
        .bind(cadence_hours)
        .bind(config)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_by_id(id: Uuid, pool: &PgPool) -> Result<Self> {
        sqlx::query_as::<_, Self>("SELECT * FROM sources WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .map_err(Into::into)
    }

    pub async fn find_all(pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>("SELECT * FROM sources ORDER BY created_at DESC")
            .fetch_all(pool)
            .await
            .map_err(Into::into)
    }

    pub async fn find_active(pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM sources WHERE is_active = TRUE ORDER BY last_scraped_at ASC NULLS FIRST",
        )
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_due_for_scrape(pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM sources
            WHERE is_active = TRUE
              AND (last_scraped_at IS NULL
                   OR last_scraped_at < NOW() - (cadence_hours || ' hours')::INTERVAL)
            ORDER BY last_scraped_at ASC NULLS FIRST
            "#,
        )
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn update_last_scraped(id: Uuid, pool: &PgPool) -> Result<()> {
        sqlx::query("UPDATE sources SET last_scraped_at = NOW() WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct WebsiteSource {
    pub id: Uuid,
    pub source_id: Uuid,
    pub domain: String,
    pub max_crawl_depth: i32,
    pub is_trusted: bool,
}

impl WebsiteSource {
    pub async fn create(
        source_id: Uuid,
        domain: &str,
        max_crawl_depth: i32,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO website_sources (source_id, domain, max_crawl_depth)
            VALUES ($1, $2, $3)
            RETURNING *
            "#,
        )
        .bind(source_id)
        .bind(domain)
        .bind(max_crawl_depth)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_by_source_id(source_id: Uuid, pool: &PgPool) -> Result<Option<Self>> {
        sqlx::query_as::<_, Self>("SELECT * FROM website_sources WHERE source_id = $1")
            .bind(source_id)
            .fetch_optional(pool)
            .await
            .map_err(Into::into)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SocialSource {
    pub id: Uuid,
    pub source_id: Uuid,
    pub platform: String,
    pub handle: String,
}

impl SocialSource {
    pub async fn create(
        source_id: Uuid,
        platform: &str,
        handle: &str,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO social_sources (source_id, platform, handle)
            VALUES ($1, $2, $3)
            RETURNING *
            "#,
        )
        .bind(source_id)
        .bind(platform)
        .bind(handle)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }
}
