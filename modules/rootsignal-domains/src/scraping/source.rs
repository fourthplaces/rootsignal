use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use url::Url;
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
    /// Returns a sensible default cadence (in hours) for a given source type.
    pub fn default_cadence_hours(source_type: &str) -> i32 {
        match source_type {
            "website" => 168,        // 1 week
            "search_query" => 24,    // 1 day
            "social" => 12,          // twice a day
            _ => 24,                 // fallback: 1 day
        }
    }

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
        let effective_cadence = cadence_hours.unwrap_or_else(|| Self::default_cadence_hours(source_type));

        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO sources (name, source_type, url, handle, entity_id, cadence_hours, config, is_active)
            VALUES ($1, $2, $3, $4, $5, $6, $7, FALSE)
            RETURNING *
            "#,
        )
        .bind(name)
        .bind(source_type)
        .bind(url)
        .bind(handle)
        .bind(entity_id)
        .bind(effective_cadence)
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

    pub async fn find_pending_qualification(pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM sources WHERE qualification_status = 'pending' ORDER BY created_at ASC",
        )
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    /// Find an existing website source by domain, or create a new one.
    /// Returns `(source, was_created)`.
    pub async fn find_or_create_website(
        name: &str,
        url: &str,
        discovered_from: Option<Uuid>,
        pool: &PgPool,
    ) -> Result<(Self, bool)> {
        let parsed = Url::parse(url)?;
        let domain = parsed
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("URL has no host: {}", url))?
            .to_string();

        // Check if a website_source with this domain already exists
        let existing = sqlx::query_as::<_, (Uuid,)>(
            "SELECT source_id FROM website_sources WHERE domain = $1",
        )
        .bind(&domain)
        .fetch_optional(pool)
        .await?;

        if let Some((source_id,)) = existing {
            let source = Self::find_by_id(source_id, pool).await?;
            return Ok((source, false));
        }

        // Create new source
        let mut config = serde_json::json!({});
        if let Some(parent_id) = discovered_from {
            config["discovered_from"] = serde_json::json!(parent_id.to_string());
        }

        let source = Self::create(name, "website", Some(url), None, None, None, config, pool).await?;

        // Create website_source record
        WebsiteSource::create(source.id, &domain, 2, pool).await?;

        Ok((source, true))
    }

    pub async fn set_entity_id(id: Uuid, entity_id: Uuid, pool: &PgPool) -> Result<()> {
        sqlx::query("UPDATE sources SET entity_id = $2 WHERE id = $1")
            .bind(id)
            .bind(entity_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn update_last_scraped(id: Uuid, pool: &PgPool) -> Result<()> {
        sqlx::query("UPDATE sources SET last_scraped_at = NOW() WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn delete_many(ids: &[Uuid], pool: &PgPool) -> Result<u64> {
        let result = sqlx::query("DELETE FROM sources WHERE id = ANY($1)")
            .bind(ids)
            .execute(pool)
            .await?;
        Ok(result.rows_affected())
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
