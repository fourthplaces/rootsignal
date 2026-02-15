use async_graphql::*;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(SimpleObject, Clone)]
pub struct GqlLocation {
    pub id: Uuid,
    pub name: Option<String>,
    pub street_address: Option<String>,
    pub address_locality: Option<String>,
    pub address_region: Option<String>,
    pub postal_code: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub location_type: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl From<rootsignal_domains::geo::Location> for GqlLocation {
    fn from(l: rootsignal_domains::geo::Location) -> Self {
        Self {
            id: l.id,
            name: l.name,
            street_address: l.street_address,
            address_locality: l.address_locality,
            address_region: l.address_region,
            postal_code: l.postal_code,
            latitude: l.latitude,
            longitude: l.longitude,
            location_type: l.location_type,
            created_at: l.created_at,
        }
    }
}
