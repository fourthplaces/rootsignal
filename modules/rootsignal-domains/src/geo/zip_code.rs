use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ZipCode {
    pub zip_code: String,
    pub address_locality: String,
    pub address_region: String,
    pub latitude: f64,
    pub longitude: f64,
}

impl ZipCode {
    pub async fn find_by_code(zip: &str, pool: &PgPool) -> Result<Option<Self>> {
        sqlx::query_as::<_, Self>("SELECT * FROM zip_codes WHERE zip_code = $1")
            .bind(zip)
            .fetch_optional(pool)
            .await
            .map_err(Into::into)
    }

    pub async fn find_by_city(city: &str, state: &str, pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM zip_codes WHERE LOWER(address_locality) = LOWER($1) AND address_region = $2",
        )
        .bind(city)
        .bind(state)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }
}
