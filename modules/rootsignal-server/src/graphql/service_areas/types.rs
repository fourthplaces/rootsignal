use async_graphql::*;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(SimpleObject, Clone)]
pub struct GqlServiceArea {
    pub id: Uuid,
    pub address_locality: String,
    pub address_region: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

impl From<rootsignal_domains::config::ServiceArea> for GqlServiceArea {
    fn from(sa: rootsignal_domains::config::ServiceArea) -> Self {
        Self {
            id: sa.id,
            address_locality: sa.address_locality,
            address_region: sa.address_region,
            is_active: sa.is_active,
            created_at: sa.created_at,
        }
    }
}
