use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Service {
    pub id: Uuid,
    pub entity_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub url: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub interpretation_services: Option<String>,
    pub application_process: Option<String>,
    pub fees_description: Option<String>,
    pub eligibility_description: Option<String>,
    pub source_locale: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Service {
    pub async fn create(
        entity_id: Uuid,
        name: &str,
        description: Option<&str>,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO services (entity_id, name, description)
            VALUES ($1, $2, $3)
            RETURNING *
            "#,
        )
        .bind(entity_id)
        .bind(name)
        .bind(description)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_by_id(id: Uuid, pool: &PgPool) -> Result<Self> {
        sqlx::query_as::<_, Self>("SELECT * FROM services WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .map_err(Into::into)
    }

    pub async fn find_by_entity_id(entity_id: Uuid, pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM services WHERE entity_id = $1 AND status = 'active' ORDER BY name",
        )
        .bind(entity_id)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    /// Find or create a service by entity + name.
    pub async fn find_or_create(
        entity_id: Uuid,
        name: &str,
        description: Option<&str>,
        pool: &PgPool,
    ) -> Result<Self> {
        if let Some(existing) = sqlx::query_as::<_, Self>(
            "SELECT * FROM services WHERE entity_id = $1 AND name = $2",
        )
        .bind(entity_id)
        .bind(name)
        .fetch_optional(pool)
        .await?
        {
            return Ok(existing);
        }
        Self::create(entity_id, name, description, pool).await
    }

    pub async fn update(
        id: Uuid,
        name: Option<&str>,
        description: Option<&str>,
        url: Option<&str>,
        email: Option<&str>,
        phone: Option<&str>,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            UPDATE services SET
                name = COALESCE($2, name),
                description = COALESCE($3, description),
                url = COALESCE($4, url),
                email = COALESCE($5, email),
                phone = COALESCE($6, phone),
                updated_at = NOW()
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(id)
        .bind(name)
        .bind(description)
        .bind(url)
        .bind(email)
        .bind(phone)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }
}
