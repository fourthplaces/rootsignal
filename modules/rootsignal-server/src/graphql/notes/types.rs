use async_graphql::*;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(SimpleObject, Clone)]
pub struct GqlNote {
    pub id: Uuid,
    pub content: String,
    pub severity: String,
    pub source_url: Option<String>,
    pub source_type: Option<String>,
    pub is_public: bool,
    pub created_by: String,
    pub expired_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl From<rootsignal_domains::shared::Note> for GqlNote {
    fn from(n: rootsignal_domains::shared::Note) -> Self {
        Self {
            id: n.id,
            content: n.content,
            severity: n.severity,
            source_url: n.source_url,
            source_type: n.source_type,
            is_public: n.is_public,
            created_by: n.created_by,
            expired_at: n.expired_at,
            created_at: n.created_at,
        }
    }
}
