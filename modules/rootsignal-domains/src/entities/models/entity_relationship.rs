use anyhow::Result;
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct EntityRelationship {
    pub id: Uuid,
    pub from_entity_id: Uuid,
    pub to_entity_id: Uuid,
    pub relationship_type: String,
    pub description: Option<String>,
    pub source: String,
    pub confidence: Option<f64>,
    pub started_at: Option<NaiveDate>,
    pub ended_at: Option<NaiveDate>,
    pub created_at: DateTime<Utc>,
}

impl EntityRelationship {
    pub async fn create(
        from_entity_id: Uuid,
        to_entity_id: Uuid,
        relationship_type: &str,
        description: Option<&str>,
        source: &str,
        confidence: Option<f64>,
        started_at: Option<NaiveDate>,
        ended_at: Option<NaiveDate>,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO entity_relationships (from_entity_id, to_entity_id, relationship_type, description, source, confidence, started_at, ended_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (from_entity_id, to_entity_id, relationship_type)
            DO UPDATE SET description = COALESCE(EXCLUDED.description, entity_relationships.description),
                         confidence = COALESCE(EXCLUDED.confidence, entity_relationships.confidence),
                         ended_at = EXCLUDED.ended_at
            RETURNING *
            "#,
        )
        .bind(from_entity_id)
        .bind(to_entity_id)
        .bind(relationship_type)
        .bind(description)
        .bind(source)
        .bind(confidence)
        .bind(started_at)
        .bind(ended_at)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_from(entity_id: Uuid, pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM entity_relationships WHERE from_entity_id = $1 ORDER BY created_at DESC",
        )
        .bind(entity_id)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_to(entity_id: Uuid, pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM entity_relationships WHERE to_entity_id = $1 ORDER BY created_at DESC",
        )
        .bind(entity_id)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_all_for_entity(entity_id: Uuid, pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM entity_relationships
            WHERE from_entity_id = $1 OR to_entity_id = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(entity_id)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_by_type(
        relationship_type: &str,
        pool: &PgPool,
    ) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM entity_relationships WHERE relationship_type = $1 ORDER BY created_at DESC",
        )
        .bind(relationship_type)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn end_relationship(id: Uuid, ended_at: NaiveDate, pool: &PgPool) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            "UPDATE entity_relationships SET ended_at = $1 WHERE id = $2 RETURNING *",
        )
        .bind(ended_at)
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }
}
