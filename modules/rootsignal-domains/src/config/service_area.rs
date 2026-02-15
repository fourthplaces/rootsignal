use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ServiceArea {
    pub id: Uuid,
    pub address_locality: String,
    pub address_region: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

impl ServiceArea {
    pub fn location_label(&self) -> String {
        format!("{}, {}", self.address_locality, self.address_region)
    }

    pub async fn create(
        address_locality: &str,
        address_region: &str,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO service_areas (address_locality, address_region)
            VALUES ($1, $2)
            RETURNING *
            "#,
        )
        .bind(address_locality)
        .bind(address_region)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_all(pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM service_areas ORDER BY address_region, address_locality",
        )
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_active(pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM service_areas WHERE is_active = TRUE ORDER BY address_region, address_locality",
        )
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn delete(id: Uuid, pool: &PgPool) -> Result<()> {
        sqlx::query("DELETE FROM service_areas WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }
}
