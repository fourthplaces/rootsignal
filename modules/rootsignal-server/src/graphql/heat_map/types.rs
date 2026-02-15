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

impl From<rootsignal_domains::heat_map::HeatMapPoint> for GqlHeatMapPoint {
    fn from(h: rootsignal_domains::heat_map::HeatMapPoint) -> Self {
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

#[derive(SimpleObject, Clone)]
pub struct GqlZipDensity {
    pub zip_code: String,
    pub address_locality: String,
    pub latitude: f64,
    pub longitude: f64,
    pub listing_count: i32,
    pub signal_domain_counts: serde_json::Value,
}

impl From<rootsignal_domains::heat_map::ZipDensity> for GqlZipDensity {
    fn from(z: rootsignal_domains::heat_map::ZipDensity) -> Self {
        Self {
            zip_code: z.zip_code,
            address_locality: z.address_locality,
            latitude: z.latitude,
            longitude: z.longitude,
            listing_count: z.listing_count as i32,
            signal_domain_counts: z.signal_domain_counts,
        }
    }
}

#[derive(SimpleObject, Clone)]
pub struct GqlTemporalDelta {
    pub zip_code: String,
    pub latitude: f64,
    pub longitude: f64,
    pub current_count: i32,
    pub previous_count: i32,
    pub delta: i32,
    pub change_pct: f64,
}

impl From<rootsignal_domains::heat_map::TemporalDelta> for GqlTemporalDelta {
    fn from(t: rootsignal_domains::heat_map::TemporalDelta) -> Self {
        Self {
            zip_code: t.zip_code,
            latitude: t.latitude,
            longitude: t.longitude,
            current_count: t.current_count as i32,
            previous_count: t.previous_count as i32,
            delta: t.delta as i32,
            change_pct: t.change_pct,
        }
    }
}
