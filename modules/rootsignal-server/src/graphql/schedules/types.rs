use async_graphql::*;
use chrono::{DateTime, NaiveTime, Utc};
use uuid::Uuid;

#[derive(SimpleObject, Clone)]
pub struct GqlSchedule {
    pub id: Uuid,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_to: Option<DateTime<Utc>>,
    pub dtstart: Option<String>,
    pub freq: Option<String>,
    pub byday: Option<String>,
    pub bymonthday: Option<String>,
    pub opens_at: Option<NaiveTime>,
    pub closes_at: Option<NaiveTime>,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl From<rootsignal_domains::shared::Schedule> for GqlSchedule {
    fn from(s: rootsignal_domains::shared::Schedule) -> Self {
        Self {
            id: s.id,
            valid_from: s.valid_from,
            valid_to: s.valid_to,
            dtstart: s.dtstart,
            freq: s.freq,
            byday: s.byday,
            bymonthday: s.bymonthday,
            opens_at: s.opens_at,
            closes_at: s.closes_at,
            description: s.description,
            created_at: s.created_at,
        }
    }
}
