use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Translation {
    pub id: Uuid,
    pub translatable_type: String,
    pub translatable_id: Uuid,
    pub field_name: String,
    pub locale: String,
    pub content: String,
    pub source_locale: Option<String>,
    pub translated_by: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl Translation {
    pub async fn create(
        translatable_type: &str,
        translatable_id: Uuid,
        field_name: &str,
        locale: &str,
        content: &str,
        source_locale: Option<&str>,
        translated_by: Option<&str>,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO translations (translatable_type, translatable_id, field_name, locale, content, source_locale, translated_by)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (translatable_type, translatable_id, field_name, locale)
            DO UPDATE SET content = EXCLUDED.content, translated_by = EXCLUDED.translated_by
            RETURNING *
            "#,
        )
        .bind(translatable_type)
        .bind(translatable_id)
        .bind(field_name)
        .bind(locale)
        .bind(content)
        .bind(source_locale)
        .bind(translated_by)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_for(
        translatable_type: &str,
        translatable_id: Uuid,
        locale: &str,
        pool: &PgPool,
    ) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM translations WHERE translatable_type = $1 AND translatable_id = $2 AND locale = $3",
        )
        .bind(translatable_type)
        .bind(translatable_id)
        .bind(locale)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_field(
        translatable_type: &str,
        translatable_id: Uuid,
        field_name: &str,
        locale: &str,
        pool: &PgPool,
    ) -> Result<Option<Self>> {
        sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM translations
            WHERE translatable_type = $1 AND translatable_id = $2 AND field_name = $3 AND locale = $4
            "#,
        )
        .bind(translatable_type)
        .bind(translatable_id)
        .bind(field_name)
        .bind(locale)
        .fetch_optional(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_all_for(
        translatable_type: &str,
        translatable_id: Uuid,
        pool: &PgPool,
    ) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM translations WHERE translatable_type = $1 AND translatable_id = $2",
        )
        .bind(translatable_type)
        .bind(translatable_id)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }
}
