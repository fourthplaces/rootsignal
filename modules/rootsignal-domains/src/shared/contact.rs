use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Contact {
    pub id: Uuid,
    pub name: Option<String>,
    pub title: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub department: Option<String>,
    pub contactable_type: String,
    pub contactable_id: Uuid,
    pub created_at: DateTime<Utc>,
}

impl Contact {
    pub async fn create(
        contactable_type: &str,
        contactable_id: Uuid,
        name: Option<&str>,
        email: Option<&str>,
        phone: Option<&str>,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO contacts (contactable_type, contactable_id, name, email, phone)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (contactable_type, contactable_id, email) DO NOTHING
            RETURNING *
            "#,
        )
        .bind(contactable_type)
        .bind(contactable_id)
        .bind(name)
        .bind(email)
        .bind(phone)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_for(
        contactable_type: &str,
        contactable_id: Uuid,
        pool: &PgPool,
    ) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM contacts WHERE contactable_type = $1 AND contactable_id = $2",
        )
        .bind(contactable_type)
        .bind(contactable_id)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }
}
