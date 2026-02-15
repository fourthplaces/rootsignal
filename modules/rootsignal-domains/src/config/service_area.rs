use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ServiceArea {
    pub id: Uuid,
    pub city: String,
    pub state: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

impl ServiceArea {
    pub fn location_label(&self) -> String {
        format!("{}, {}", self.city, self.state)
    }

    pub async fn create(city: &str, state: &str, pool: &PgPool) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO service_areas (city, state)
            VALUES ($1, $2)
            RETURNING *
            "#,
        )
        .bind(city)
        .bind(state)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_all(pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>("SELECT * FROM service_areas ORDER BY state, city")
            .fetch_all(pool)
            .await
            .map_err(Into::into)
    }

    pub async fn find_active(pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM service_areas WHERE is_active = TRUE ORDER BY state, city",
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
