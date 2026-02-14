use async_graphql::*;

use super::super::tags::types::GqlTagCount;

#[derive(SimpleObject)]
pub struct GqlListingStats {
    pub total_listings: i64,
    pub active_listings: i64,
    pub total_sources: i64,
    pub total_snapshots: i64,
    pub total_extractions: i64,
    pub total_entities: i64,
    pub listings_by_type: Vec<GqlTagCount>,
    pub listings_by_role: Vec<GqlTagCount>,
    pub listings_by_category: Vec<GqlTagCount>,
    pub listings_by_domain: Vec<GqlTagCount>,
    pub listings_by_urgency: Vec<GqlTagCount>,
    pub listings_by_confidence: Vec<GqlTagCount>,
    pub listings_by_capacity: Vec<GqlTagCount>,
    pub recent_7d: i64,
}

impl From<taproot_domains::listings::ListingStats> for GqlListingStats {
    fn from(s: taproot_domains::listings::ListingStats) -> Self {
        Self {
            total_listings: s.total_listings,
            active_listings: s.active_listings,
            total_sources: s.total_sources,
            total_snapshots: s.total_snapshots,
            total_extractions: s.total_extractions,
            total_entities: s.total_entities,
            listings_by_type: s.listings_by_type.into_iter().map(GqlTagCount::from).collect(),
            listings_by_role: s.listings_by_role.into_iter().map(GqlTagCount::from).collect(),
            listings_by_category: s.listings_by_category.into_iter().map(GqlTagCount::from).collect(),
            listings_by_domain: s.listings_by_domain.into_iter().map(GqlTagCount::from).collect(),
            listings_by_urgency: s.listings_by_urgency.into_iter().map(GqlTagCount::from).collect(),
            listings_by_confidence: s.listings_by_confidence.into_iter().map(GqlTagCount::from).collect(),
            listings_by_capacity: s.listings_by_capacity.into_iter().map(GqlTagCount::from).collect(),
            recent_7d: s.recent_7d,
        }
    }
}
