use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Hotspot {
    pub id: Uuid,
    pub name: String,
    pub center_lat: f64,
    pub center_lng: f64,
    pub radius_meters: i32,
    pub hotspot_type: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

impl Hotspot {
    pub async fn find_active(pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM hotspots WHERE is_active = TRUE ORDER BY name",
        )
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_by_id(id: Uuid, pool: &PgPool) -> Result<Self> {
        sqlx::query_as::<_, Self>("SELECT * FROM hotspots WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .map_err(Into::into)
    }
}
