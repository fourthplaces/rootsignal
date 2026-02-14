use anyhow::Result;
use chrono::{DateTime, Utc};
use pgvector::Vector;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Embedding {
    pub id: Uuid,
    pub embeddable_type: String,
    pub embeddable_id: Uuid,
    pub locale: String,
    pub embedding: Vector,
    pub source_text_hash: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Row returned from similarity search.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SimilarRecord {
    pub embeddable_type: String,
    pub embeddable_id: Uuid,
    pub distance: f64,
}

impl Embedding {
    pub async fn upsert(
        embeddable_type: &str,
        embeddable_id: Uuid,
        locale: &str,
        embedding: Vector,
        source_text_hash: &str,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO embeddings (embeddable_type, embeddable_id, locale, embedding, source_text_hash)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (embeddable_type, embeddable_id, locale)
            DO UPDATE SET embedding = EXCLUDED.embedding, source_text_hash = EXCLUDED.source_text_hash
            RETURNING *
            "#,
        )
        .bind(embeddable_type)
        .bind(embeddable_id)
        .bind(locale)
        .bind(&embedding)
        .bind(source_text_hash)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_for(
        embeddable_type: &str,
        embeddable_id: Uuid,
        locale: &str,
        pool: &PgPool,
    ) -> Result<Option<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM embeddings WHERE embeddable_type = $1 AND embeddable_id = $2 AND locale = $3",
        )
        .bind(embeddable_type)
        .bind(embeddable_id)
        .bind(locale)
        .fetch_optional(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn search_similar(
        query_embedding: Vector,
        embeddable_type: &str,
        limit: i64,
        threshold: f64,
        pool: &PgPool,
    ) -> Result<Vec<SimilarRecord>> {
        sqlx::query_as::<_, SimilarRecord>(
            r#"
            SELECT embeddable_type, embeddable_id, (embedding <=> $1) as distance
            FROM embeddings
            WHERE locale = 'en' AND embeddable_type = $2 AND (embedding <=> $1) < $3
            ORDER BY embedding <=> $1
            LIMIT $4
            "#,
        )
        .bind(&query_embedding)
        .bind(embeddable_type)
        .bind(threshold)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn get_hash(
        embeddable_type: &str,
        embeddable_id: Uuid,
        locale: &str,
        pool: &PgPool,
    ) -> Result<Option<String>> {
        let row = sqlx::query_as::<_, (Option<String>,)>(
            "SELECT source_text_hash FROM embeddings WHERE embeddable_type = $1 AND embeddable_id = $2 AND locale = $3",
        )
        .bind(embeddable_type)
        .bind(embeddable_id)
        .bind(locale)
        .fetch_optional(pool)
        .await?;
        Ok(row.and_then(|r| r.0))
    }
}
