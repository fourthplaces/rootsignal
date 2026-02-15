use async_graphql::*;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(SimpleObject, Clone)]
pub struct GqlTag {
    pub id: Uuid,
    pub kind: String,
    pub value: String,
    pub display_name: Option<String>,
}

impl From<rootsignal_domains::taxonomy::Tag> for GqlTag {
    fn from(t: rootsignal_domains::taxonomy::Tag) -> Self {
        Self {
            id: t.id,
            kind: t.kind,
            value: t.value,
            display_name: t.display_name,
        }
    }
}

#[derive(SimpleObject, Clone)]
pub struct GqlTagKind {
    pub id: Uuid,
    pub slug: String,
    pub display_name: String,
    pub description: Option<String>,
    pub allowed_resource_types: Vec<String>,
    pub required: bool,
    pub is_public: bool,
    pub created_at: DateTime<Utc>,
}

impl From<rootsignal_domains::taxonomy::TagKindConfig> for GqlTagKind {
    fn from(t: rootsignal_domains::taxonomy::TagKindConfig) -> Self {
        Self {
            id: t.id,
            slug: t.slug,
            display_name: t.display_name,
            description: t.description,
            allowed_resource_types: t.allowed_resource_types,
            required: t.required,
            is_public: t.is_public,
            created_at: t.created_at,
        }
    }
}

#[derive(SimpleObject, Clone)]
pub struct GqlTagCount {
    pub value: String,
    pub count: i64,
}

impl From<rootsignal_domains::listings::TagCount> for GqlTagCount {
    fn from(t: rootsignal_domains::listings::TagCount) -> Self {
        Self {
            value: t.value,
            count: t.count,
        }
    }
}
