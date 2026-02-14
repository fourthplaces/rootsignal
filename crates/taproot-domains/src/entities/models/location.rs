use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Location {
    pub id: Uuid,
    pub entity_id: Option<Uuid>,
    pub name: Option<String>,
    pub address_line_1: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub postal_code: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub location_type: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl Location {
    pub async fn create(
        entity_id: Option<Uuid>,
        name: Option<&str>,
        address_line_1: Option<&str>,
        city: Option<&str>,
        state: Option<&str>,
        postal_code: Option<&str>,
        latitude: Option<f64>,
        longitude: Option<f64>,
        location_type: Option<&str>,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO locations (entity_id, name, address_line_1, city, state, postal_code, latitude, longitude, location_type)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING *
            "#,
        )
        .bind(entity_id)
        .bind(name)
        .bind(address_line_1)
        .bind(city)
        .bind(state)
        .bind(postal_code)
        .bind(latitude)
        .bind(longitude)
        .bind(location_type)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    /// Find or create a location from extraction data, auto-resolving lat/lng from zip_codes.
    pub async fn find_or_create_from_extraction(
        city: Option<&str>,
        state: Option<&str>,
        postal_code: Option<&str>,
        address: Option<&str>,
        pool: &PgPool,
    ) -> Result<Self> {
        // Try to find existing location by postal code
        if let Some(zip) = postal_code {
            let existing = sqlx::query_as::<_, Self>(
                "SELECT * FROM locations WHERE postal_code = $1 LIMIT 1",
            )
            .bind(zip)
            .fetch_optional(pool)
            .await?;

            if let Some(loc) = existing {
                return Ok(loc);
            }
        }

        // Resolve lat/lng from zip_codes table
        let (lat, lng) = if let Some(zip) = postal_code {
            let coords = sqlx::query_as::<_, (f64, f64)>(
                "SELECT latitude, longitude FROM zip_codes WHERE zip_code = $1",
            )
            .bind(zip)
            .fetch_optional(pool)
            .await?;
            match coords {
                Some((lat, lng)) => (Some(lat), Some(lng)),
                None => (None, None),
            }
        } else if let (Some(c), Some(s)) = (city, state) {
            // Fallback: look up by city/state, take first match
            let coords = sqlx::query_as::<_, (f64, f64)>(
                "SELECT latitude, longitude FROM zip_codes WHERE LOWER(city) = LOWER($1) AND state = $2 LIMIT 1",
            )
            .bind(c)
            .bind(s)
            .fetch_optional(pool)
            .await?;
            match coords {
                Some((lat, lng)) => (Some(lat), Some(lng)),
                None => (None, None),
            }
        } else {
            (None, None)
        };

        Self::create(
            None,
            None,
            address,
            city,
            state,
            postal_code,
            lat,
            lng,
            Some("physical"),
            pool,
        )
        .await
    }

    pub async fn find_by_id(id: Uuid, pool: &PgPool) -> Result<Self> {
        sqlx::query_as::<_, Self>("SELECT * FROM locations WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .map_err(Into::into)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Locationable {
    pub id: Uuid,
    pub location_id: Uuid,
    pub locatable_type: String,
    pub locatable_id: Uuid,
    pub is_primary: bool,
}

impl Locationable {
    pub async fn create(
        location_id: Uuid,
        locatable_type: &str,
        locatable_id: Uuid,
        is_primary: bool,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO locationables (location_id, locatable_type, locatable_id, is_primary)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (location_id, locatable_type, locatable_id) DO NOTHING
            RETURNING *
            "#,
        )
        .bind(location_id)
        .bind(locatable_type)
        .bind(locatable_id)
        .bind(is_primary)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_for(
        locatable_type: &str,
        locatable_id: Uuid,
        pool: &PgPool,
    ) -> Result<Vec<Location>> {
        sqlx::query_as::<_, Location>(
            r#"
            SELECT l.* FROM locations l
            JOIN locationables la ON la.location_id = l.id
            WHERE la.locatable_type = $1 AND la.locatable_id = $2
            "#,
        )
        .bind(locatable_type)
        .bind(locatable_id)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }
}
