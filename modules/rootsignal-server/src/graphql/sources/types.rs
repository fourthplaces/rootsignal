use async_graphql::*;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(SimpleObject, Clone)]
#[graphql(complex)]
pub struct GqlSource {
    pub id: Uuid,
    pub entity_id: Option<Uuid>,
    pub name: String,
    pub source_type: String,
    pub url: Option<String>,
    pub handle: Option<String>,
    pub next_run_at: Option<DateTime<Utc>>,
    pub consecutive_misses: i32,
    pub last_scraped_at: Option<DateTime<Utc>>,
    pub is_active: bool,
    pub config: serde_json::Value,
    pub content_summary: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[ComplexObject]
impl GqlSource {
    /// Number of signals extracted from this source's snapshots.
    async fn signal_count(&self, ctx: &Context<'_>) -> Result<i32> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let row = sqlx::query_as::<_, (i64,)>(
            r#"
            SELECT COUNT(*)
            FROM signals s
            JOIN domain_snapshots ds ON ds.page_snapshot_id = s.page_snapshot_id
            WHERE ds.source_id = $1
            "#,
        )
        .bind(self.id)
        .fetch_one(pool)
        .await
        .unwrap_or((0,));
        Ok(row.0 as i32)
    }
}

#[derive(SimpleObject, Clone)]
pub struct GqlPageSnapshot {
    pub id: Uuid,
    pub page_url: String,
    pub url: String,
    pub content_hash: String,
    pub fetched_via: String,
    pub content_preview: Option<String>,
    pub crawled_at: DateTime<Utc>,
    pub scrape_status: String,
}

#[derive(SimpleObject, Clone)]
pub struct GqlPageSnapshotDetail {
    pub id: Uuid,
    pub source_id: Option<Uuid>,
    pub url: String,
    pub canonical_url: String,
    pub content_hash: String,
    pub fetched_via: String,
    pub html: Option<String>,
    pub content: Option<String>,
    pub metadata: serde_json::Value,
    pub crawled_at: DateTime<Utc>,
    pub extraction_status: String,
    pub extraction_completed_at: Option<DateTime<Utc>>,
}

impl From<rootsignal_domains::scraping::Source> for GqlSource {
    fn from(s: rootsignal_domains::scraping::Source) -> Self {
        let cadence = chrono::Duration::hours(s.effective_cadence_hours() as i64);
        let next_run_at = s.last_scraped_at.map(|last| last + cadence);
        Self {
            id: s.id,
            entity_id: s.entity_id,
            name: s.name,
            source_type: s.source_type,
            url: s.url,
            handle: s.handle,
            next_run_at,
            consecutive_misses: s.consecutive_misses,
            last_scraped_at: s.last_scraped_at,
            is_active: s.is_active,
            config: s.config,
            content_summary: s.content_summary,
            created_at: s.created_at,
        }
    }
}
