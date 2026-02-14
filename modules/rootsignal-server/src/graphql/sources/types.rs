use async_graphql::*;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Source type with `config` field excluded for security.
#[allow(dead_code)]
#[derive(SimpleObject, Clone)]
pub struct GqlSource {
    pub id: Uuid,
    pub entity_id: Option<Uuid>,
    pub name: String,
    pub source_type: String,
    pub adapter: String,
    pub url: Option<String>,
    pub handle: Option<String>,
    pub cadence_hours: i32,
    pub last_scraped_at: Option<DateTime<Utc>>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

impl From<rootsignal_domains::entities::Source> for GqlSource {
    fn from(s: rootsignal_domains::entities::Source) -> Self {
        Self {
            id: s.id,
            entity_id: s.entity_id,
            name: s.name,
            source_type: s.source_type,
            adapter: s.adapter,
            url: s.url,
            handle: s.handle,
            cadence_hours: s.cadence_hours,
            last_scraped_at: s.last_scraped_at,
            is_active: s.is_active,
            created_at: s.created_at,
        }
    }
}
