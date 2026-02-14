use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Entity {
    pub id: Uuid,
    pub name: String,
    pub entity_type: String,
    pub description: Option<String>,
    pub website: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub verified: bool,
    pub source_locale: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Entity {
    pub async fn create(
        name: &str,
        entity_type: &str,
        description: Option<&str>,
        website: Option<&str>,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO entities (name, entity_type, description, website)
            VALUES ($1, $2, $3, $4)
            RETURNING *
            "#,
        )
        .bind(name)
        .bind(entity_type)
        .bind(description)
        .bind(website)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_by_id(id: Uuid, pool: &PgPool) -> Result<Self> {
        sqlx::query_as::<_, Self>("SELECT * FROM entities WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .map_err(Into::into)
    }

    pub async fn find_by_name_and_type(
        name: &str,
        entity_type: &str,
        pool: &PgPool,
    ) -> Result<Option<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM entities WHERE name = $1 AND entity_type = $2",
        )
        .bind(name)
        .bind(entity_type)
        .fetch_optional(pool)
        .await
        .map_err(Into::into)
    }

    /// Find or create an entity by name and type.
    pub async fn find_or_create(
        name: &str,
        entity_type: &str,
        description: Option<&str>,
        website: Option<&str>,
        pool: &PgPool,
    ) -> Result<Self> {
        if let Some(existing) = Self::find_by_name_and_type(name, entity_type, pool).await? {
            return Ok(existing);
        }
        Self::create(name, entity_type, description, website, pool).await
    }

    pub async fn list_all(pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>("SELECT * FROM entities ORDER BY name ASC")
            .fetch_all(pool)
            .await
            .map_err(Into::into)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Organization {
    pub id: Uuid,
    pub entity_id: Uuid,
    pub organization_type: Option<String>,
    pub ein: Option<String>,
    pub mission: Option<String>,
}

impl Organization {
    pub async fn create(
        entity_id: Uuid,
        organization_type: Option<&str>,
        mission: Option<&str>,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO organizations (entity_id, organization_type, mission)
            VALUES ($1, $2, $3)
            RETURNING *
            "#,
        )
        .bind(entity_id)
        .bind(organization_type)
        .bind(mission)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_by_entity_id(entity_id: Uuid, pool: &PgPool) -> Result<Option<Self>> {
        sqlx::query_as::<_, Self>("SELECT * FROM organizations WHERE entity_id = $1")
            .bind(entity_id)
            .fetch_optional(pool)
            .await
            .map_err(Into::into)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct GovernmentEntity {
    pub id: Uuid,
    pub entity_id: Uuid,
    pub jurisdiction: Option<String>,
    pub agency_type: Option<String>,
    pub jurisdiction_name: Option<String>,
}

impl GovernmentEntity {
    pub async fn create(
        entity_id: Uuid,
        jurisdiction: Option<&str>,
        agency_type: Option<&str>,
        jurisdiction_name: Option<&str>,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO government_entities (entity_id, jurisdiction, agency_type, jurisdiction_name)
            VALUES ($1, $2, $3, $4)
            RETURNING *
            "#,
        )
        .bind(entity_id)
        .bind(jurisdiction)
        .bind(agency_type)
        .bind(jurisdiction_name)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct BusinessEntity {
    pub id: Uuid,
    pub entity_id: Uuid,
    pub industry: Option<String>,
    pub is_cooperative: bool,
    pub is_b_corp: bool,
}

impl BusinessEntity {
    pub async fn create(
        entity_id: Uuid,
        industry: Option<&str>,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO business_entities (entity_id, industry)
            VALUES ($1, $2)
            RETURNING *
            "#,
        )
        .bind(entity_id)
        .bind(industry)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }
}
