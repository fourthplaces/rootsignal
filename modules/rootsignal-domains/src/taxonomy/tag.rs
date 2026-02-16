use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Tag {
    pub id: Uuid,
    pub kind: String,
    pub value: String,
    pub display_name: Option<String>,
}

impl Tag {
    pub async fn find_or_create(kind: &str, value: &str, pool: &PgPool) -> Result<Self> {
        // Try insert, on conflict return existing
        let tag = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO tags (kind, value)
            VALUES ($1, $2)
            ON CONFLICT (kind, value) DO UPDATE SET kind = EXCLUDED.kind
            RETURNING *
            "#,
        )
        .bind(kind)
        .bind(value)
        .fetch_one(pool)
        .await?;
        Ok(tag)
    }

    pub async fn find_by_kind(kind: &str, pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>("SELECT * FROM tags WHERE kind = $1 ORDER BY value")
            .bind(kind)
            .fetch_all(pool)
            .await
            .map_err(Into::into)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Taggable {
    pub id: Uuid,
    pub tag_id: Uuid,
    pub taggable_type: String,
    pub taggable_id: Uuid,
}

impl Taggable {
    pub async fn create(
        tag_id: Uuid,
        taggable_type: &str,
        taggable_id: Uuid,
        pool: &PgPool,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO taggables (tag_id, taggable_type, taggable_id)
            VALUES ($1, $2, $3)
            ON CONFLICT (tag_id, taggable_type, taggable_id) DO NOTHING
            "#,
        )
        .bind(tag_id)
        .bind(taggable_type)
        .bind(taggable_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Tag an entity/signal with a kind+value pair (find-or-create the tag).
    pub async fn tag(
        taggable_type: &str,
        taggable_id: Uuid,
        kind: &str,
        value: &str,
        pool: &PgPool,
    ) -> Result<()> {
        let tag = Tag::find_or_create(kind, value, pool).await?;
        Self::create(tag.id, taggable_type, taggable_id, pool).await
    }

    pub async fn tags_for(
        taggable_type: &str,
        taggable_id: Uuid,
        pool: &PgPool,
    ) -> Result<Vec<Tag>> {
        sqlx::query_as::<_, Tag>(
            r#"
            SELECT t.* FROM tags t
            JOIN taggables tb ON tb.tag_id = t.id
            WHERE tb.taggable_type = $1 AND tb.taggable_id = $2
            "#,
        )
        .bind(taggable_type)
        .bind(taggable_id)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }
}
