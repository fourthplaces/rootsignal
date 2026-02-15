use async_graphql::*;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(SimpleObject, Clone)]
pub struct GqlSource {
    pub id: Uuid,
    pub entity_id: Option<Uuid>,
    pub name: String,
    pub source_type: String,
    pub url: Option<String>,
    pub handle: Option<String>,
    pub cadence_hours: i32,
    pub last_scraped_at: Option<DateTime<Utc>>,
    pub is_active: bool,
    pub config: serde_json::Value,
    pub qualification_status: String,
    pub qualification_summary: Option<String>,
    pub qualification_score: Option<i32>,
    pub created_at: DateTime<Utc>,
}

impl From<rootsignal_domains::scraping::Source> for GqlSource {
    fn from(s: rootsignal_domains::scraping::Source) -> Self {
        Self {
            id: s.id,
            entity_id: s.entity_id,
            name: s.name,
            source_type: s.source_type,
            url: s.url,
            handle: s.handle,
            cadence_hours: s.cadence_hours,
            last_scraped_at: s.last_scraped_at,
            is_active: s.is_active,
            config: s.config,
            qualification_status: s.qualification_status,
            qualification_summary: s.qualification_summary,
            qualification_score: s.qualification_score,
            created_at: s.created_at,
        }
    }
}
