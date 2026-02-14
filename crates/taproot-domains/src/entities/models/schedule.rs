use anyhow::Result;
use chrono::{DateTime, NaiveTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Schedule {
    pub id: Uuid,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_to: Option<DateTime<Utc>>,
    pub dtstart: Option<String>,
    pub freq: Option<String>,
    pub byday: Option<String>,
    pub bymonthday: Option<String>,
    pub opens_at: Option<NaiveTime>,
    pub closes_at: Option<NaiveTime>,
    pub description: Option<String>,
    pub scheduleable_type: String,
    pub scheduleable_id: Uuid,
    pub created_at: DateTime<Utc>,
}

impl Schedule {
    pub async fn create(
        scheduleable_type: &str,
        scheduleable_id: Uuid,
        dtstart: Option<&str>,
        freq: Option<&str>,
        byday: Option<&str>,
        bymonthday: Option<&str>,
        description: Option<&str>,
        valid_from: Option<DateTime<Utc>>,
        valid_to: Option<DateTime<Utc>>,
        opens_at: Option<NaiveTime>,
        closes_at: Option<NaiveTime>,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO schedules (scheduleable_type, scheduleable_id, dtstart, freq, byday, bymonthday, description, valid_from, valid_to, opens_at, closes_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            ON CONFLICT (scheduleable_type, scheduleable_id, dtstart) DO NOTHING
            RETURNING *
            "#,
        )
        .bind(scheduleable_type)
        .bind(scheduleable_id)
        .bind(dtstart)
        .bind(freq)
        .bind(byday)
        .bind(bymonthday)
        .bind(description)
        .bind(valid_from)
        .bind(valid_to)
        .bind(opens_at)
        .bind(closes_at)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_for(
        scheduleable_type: &str,
        scheduleable_id: Uuid,
        pool: &PgPool,
    ) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM schedules WHERE scheduleable_type = $1 AND scheduleable_id = $2",
        )
        .bind(scheduleable_type)
        .bind(scheduleable_id)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }
}
