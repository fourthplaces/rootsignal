use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Media {
    pub id: Uuid,
    pub url: String,
    pub media_type: String,
    pub content_type: Option<String>,
    pub alt_text: Option<String>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub file_size_bytes: Option<i64>,
    pub source_url: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct MediaAttachment {
    pub id: Uuid,
    pub media_id: Uuid,
    pub attachable_type: String,
    pub attachable_id: Uuid,
    pub is_primary: bool,
    pub sort_order: i32,
}

impl Media {
    pub async fn create(
        url: &str,
        media_type: &str,
        content_type: Option<&str>,
        alt_text: Option<&str>,
        width: Option<i32>,
        height: Option<i32>,
        file_size_bytes: Option<i64>,
        source_url: Option<&str>,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO media (url, media_type, content_type, alt_text, width, height, file_size_bytes, source_url)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING *
            "#,
        )
        .bind(url)
        .bind(media_type)
        .bind(content_type)
        .bind(alt_text)
        .bind(width)
        .bind(height)
        .bind(file_size_bytes)
        .bind(source_url)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_by_id(id: Uuid, pool: &PgPool) -> Result<Option<Self>> {
        sqlx::query_as::<_, Self>("SELECT * FROM media WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(Into::into)
    }

    pub async fn find_for(
        attachable_type: &str,
        attachable_id: Uuid,
        pool: &PgPool,
    ) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            r#"
            SELECT m.* FROM media m
            JOIN media_attachments ma ON ma.media_id = m.id
            WHERE ma.attachable_type = $1 AND ma.attachable_id = $2
            ORDER BY ma.sort_order ASC
            "#,
        )
        .bind(attachable_type)
        .bind(attachable_id)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_primary(
        attachable_type: &str,
        attachable_id: Uuid,
        pool: &PgPool,
    ) -> Result<Option<Self>> {
        sqlx::query_as::<_, Self>(
            r#"
            SELECT m.* FROM media m
            JOIN media_attachments ma ON ma.media_id = m.id
            WHERE ma.attachable_type = $1 AND ma.attachable_id = $2 AND ma.is_primary = TRUE
            LIMIT 1
            "#,
        )
        .bind(attachable_type)
        .bind(attachable_id)
        .fetch_optional(pool)
        .await
        .map_err(Into::into)
    }
}

impl MediaAttachment {
    pub async fn create(
        media_id: Uuid,
        attachable_type: &str,
        attachable_id: Uuid,
        is_primary: bool,
        sort_order: i32,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO media_attachments (media_id, attachable_type, attachable_id, is_primary, sort_order)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING *
            "#,
        )
        .bind(media_id)
        .bind(attachable_type)
        .bind(attachable_id)
        .bind(is_primary)
        .bind(sort_order)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn set_primary(
        media_id: Uuid,
        attachable_type: &str,
        attachable_id: Uuid,
        pool: &PgPool,
    ) -> Result<()> {
        // Clear existing primary
        sqlx::query(
            "UPDATE media_attachments SET is_primary = FALSE WHERE attachable_type = $1 AND attachable_id = $2",
        )
        .bind(attachable_type)
        .bind(attachable_id)
        .execute(pool)
        .await?;

        // Set new primary
        sqlx::query(
            "UPDATE media_attachments SET is_primary = TRUE WHERE media_id = $1 AND attachable_type = $2 AND attachable_id = $3",
        )
        .bind(media_id)
        .bind(attachable_type)
        .bind(attachable_id)
        .execute(pool)
        .await?;

        Ok(())
    }
}
