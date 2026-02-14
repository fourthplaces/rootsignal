use async_graphql::*;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(SimpleObject, Clone)]
pub struct GqlHeatMapPoint {
    pub id: Uuid,
    pub latitude: f64,
    pub longitude: f64,
    pub weight: f64,
    pub entity_type: String,
    pub entity_id: Uuid,
    pub generated_at: DateTime<Utc>,
}

impl From<taproot_domains::heat_map::HeatMapPoint> for GqlHeatMapPoint {
    fn from(h: taproot_domains::heat_map::HeatMapPoint) -> Self {
        Self {
            id: h.id,
            latitude: h.latitude,
            longitude: h.longitude,
            weight: h.weight,
            entity_type: h.entity_type,
            entity_id: h.entity_id,
            generated_at: h.generated_at,
        }
    }
}
