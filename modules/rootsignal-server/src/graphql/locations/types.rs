use async_graphql::*;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(SimpleObject, Clone)]
pub struct GqlLocation {
    pub id: Uuid,
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

impl From<rootsignal_domains::entities::Location> for GqlLocation {
    fn from(l: rootsignal_domains::entities::Location) -> Self {
        Self {
            id: l.id,
            name: l.name,
            address_line_1: l.address_line_1,
            city: l.city,
            state: l.state,
            postal_code: l.postal_code,
            latitude: l.latitude,
            longitude: l.longitude,
            location_type: l.location_type,
            created_at: l.created_at,
        }
    }
}
