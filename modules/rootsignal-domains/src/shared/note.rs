use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Note {
    pub id: Uuid,
    pub content: String,
    pub severity: String,
    pub source_url: Option<String>,
    pub source_type: Option<String>,
    pub source_id: Option<Uuid>,
    pub is_public: bool,
    pub created_by: String,
    pub expired_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Note {
    pub async fn create(
        content: &str,
        severity: &str,
        source_type: Option<&str>,
        created_by: &str,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO notes (content, severity, source_type, created_by)
            VALUES ($1, $2, $3, $4)
            RETURNING *
            "#,
        )
        .bind(content)
        .bind(severity)
        .bind(source_type)
        .bind(created_by)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn active_for(
        noteable_type: &str,
        noteable_id: Uuid,
        pool: &PgPool,
    ) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            r#"
            SELECT n.* FROM notes n
            JOIN noteables nb ON nb.note_id = n.id
            WHERE nb.noteable_type = $1 AND nb.noteable_id = $2
              AND (n.expired_at IS NULL OR n.expired_at > NOW())
            ORDER BY n.created_at DESC
            "#,
        )
        .bind(noteable_type)
        .bind(noteable_id)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Notable {
    pub id: Uuid,
    pub note_id: Uuid,
    pub noteable_type: String,
    pub noteable_id: Uuid,
}

impl Notable {
    pub async fn create(
        note_id: Uuid,
        noteable_type: &str,
        noteable_id: Uuid,
        pool: &PgPool,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO noteables (note_id, noteable_type, noteable_id)
            VALUES ($1, $2, $3)
            ON CONFLICT (note_id, noteable_type, noteable_id) DO NOTHING
            "#,
        )
        .bind(note_id)
        .bind(noteable_type)
        .bind(noteable_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Create a note and attach it to an entity in one call.
    pub async fn attach_note(
        noteable_type: &str,
        noteable_id: Uuid,
        content: &str,
        severity: &str,
        source_type: Option<&str>,
        created_by: &str,
        pool: &PgPool,
    ) -> Result<Note> {
        let note = Note::create(content, severity, source_type, created_by, pool).await?;
        Self::create(note.id, noteable_type, noteable_id, pool).await?;
        Ok(note)
    }
}
