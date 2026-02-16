use async_graphql::*;

use super::super::tags::types::GqlTagCount;

#[derive(SimpleObject)]
pub struct GqlSignalStats {
    pub total_signals: i64,
    pub total_sources: i64,
    pub total_snapshots: i64,
    pub total_extractions: i64,
    pub total_entities: i64,
    pub signals_by_type: Vec<GqlTagCount>,
    pub signals_by_domain: Vec<GqlTagCount>,
    pub recent_7d: i64,
}

impl From<rootsignal_domains::signals::SignalStats> for GqlSignalStats {
    fn from(s: rootsignal_domains::signals::SignalStats) -> Self {
        Self {
            total_signals: s.total_signals,
            total_sources: s.total_sources,
            total_snapshots: s.total_snapshots,
            total_extractions: s.total_extractions,
            total_entities: s.total_entities,
            signals_by_type: s
                .signals_by_type
                .into_iter()
                .map(GqlTagCount::from)
                .collect(),
            signals_by_domain: s
                .signals_by_domain
                .into_iter()
                .map(GqlTagCount::from)
                .collect(),
            recent_7d: s.recent_7d,
        }
    }
}
