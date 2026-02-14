use async_graphql::*;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[allow(dead_code)]
#[derive(SimpleObject, Clone)]
pub struct GqlHotspot {
    pub id: Uuid,
    pub name: String,
    pub center_lat: f64,
    pub center_lng: f64,
    pub radius_meters: i32,
    pub hotspot_type: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

impl From<taproot_domains::entities::Hotspot> for GqlHotspot {
    fn from(h: taproot_domains::entities::Hotspot) -> Self {
        Self {
            id: h.id,
            name: h.name,
            center_lat: h.center_lat,
            center_lng: h.center_lng,
            radius_meters: h.radius_meters,
            hotspot_type: h.hotspot_type,
            is_active: h.is_active,
            created_at: h.created_at,
        }
    }
}
