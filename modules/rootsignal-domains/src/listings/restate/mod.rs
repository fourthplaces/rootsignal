use restate_sdk::prelude::*;
use rootsignal_core::ServerDeps;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::listings::models::listing::{Listing, ListingFilters, ListingStats, TagCount};
use crate::taxonomy::TagKindConfig;

// ─── Request / Response types ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListListingsRequest {
    // Tag-based filters
    pub signal_domain: Option<String>,
    pub audience_role: Option<String>,
    pub category: Option<String>,
    pub listing_type: Option<String>,
    pub urgency: Option<String>,
    pub confidence: Option<String>,
    pub capacity_status: Option<String>,
    pub radius_relevant: Option<String>,
    pub population: Option<String>,
    // Geo
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub radius_km: Option<f64>,
    pub hotspot_id: Option<String>,
    // Temporal
    pub since: Option<String>,
    // Pagination
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}
impl_restate_serde!(ListListingsRequest);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListingListResult {
    pub listings: Vec<ListingJson>,
    pub count: usize,
}
impl_restate_serde!(ListingListResult);

/// JSON-safe listing (Uuid as String for Restate transport).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListingJson {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub source_url: Option<String>,
    pub location_text: Option<String>,
    pub in_language: String,
    pub relevance_score: Option<i32>,
    pub created_at: String,
}

impl From<Listing> for ListingJson {
    fn from(l: Listing) -> Self {
        Self {
            id: l.id.to_string(),
            title: l.title,
            description: l.description,
            status: l.status,
            source_url: l.source_url,
            location_text: l.location_text,
            in_language: l.in_language,
            relevance_score: l.relevance_score,
            created_at: l.created_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmptyRequest {}
impl_restate_serde!(EmptyRequest);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListingFiltersResult {
    pub kinds: Vec<FilterKindResult>,
}
impl_restate_serde!(ListingFiltersResult);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterKindResult {
    pub slug: String,
    pub display_name: String,
    pub required: bool,
    pub values: Vec<TagCount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListingStatsResult {
    pub stats: ListingStats,
}
impl_restate_serde!(ListingStatsResult);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreBatchRequest {
    pub listing_ids: Vec<String>,
}
impl_restate_serde!(ScoreBatchRequest);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreBatchResult {
    pub scored: u32,
}
impl_restate_serde!(ScoreBatchResult);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpireResult {
    pub expired: u32,
}
impl_restate_serde!(ExpireResult);

// ─── ListingsService ────────────────────────────────────────────────────────

#[restate_sdk::service]
#[name = "Listings"]
pub trait ListingsService {
    async fn list(req: ListListingsRequest) -> Result<ListingListResult, HandlerError>;
    async fn filters(req: EmptyRequest) -> Result<ListingFiltersResult, HandlerError>;
    async fn stats(req: EmptyRequest) -> Result<ListingStatsResult, HandlerError>;
    async fn score_batch(req: ScoreBatchRequest) -> Result<ScoreBatchResult, HandlerError>;
    async fn expire_stale(req: EmptyRequest) -> Result<ExpireResult, HandlerError>;
}

pub struct ListingsServiceImpl {
    deps: Arc<ServerDeps>,
}

impl ListingsServiceImpl {
    pub fn with_deps(deps: Arc<ServerDeps>) -> Self {
        Self { deps }
    }
}

impl ListingsService for ListingsServiceImpl {
    async fn list(
        &self,
        _ctx: Context<'_>,
        req: ListListingsRequest,
    ) -> Result<ListingListResult, HandlerError> {
        let pool = self.deps.pool();

        let hotspot_id = req.hotspot_id.as_ref().and_then(|s| s.parse().ok());

        let since = req
            .since
            .as_ref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc));

        let filters = ListingFilters {
            signal_domain: req.signal_domain,
            audience_role: req.audience_role,
            category: req.category,
            listing_type: req.listing_type,
            urgency: req.urgency,
            confidence: req.confidence,
            capacity_status: req.capacity_status,
            radius_relevant: req.radius_relevant,
            population: req.population,
            lat: req.lat,
            lng: req.lng,
            radius_km: req.radius_km,
            hotspot_id,
            since,
            zip_code: None,
            radius_miles: None,
            limit: req.limit,
            offset: req.offset,
        };

        let listings = Listing::find_filtered(&filters, pool)
            .await
            .map_err(|e| TerminalError::new(format!("Query failed: {}", e)))?;

        let count = listings.len();
        let json_listings: Vec<ListingJson> = listings.into_iter().map(Into::into).collect();

        Ok(ListingListResult {
            listings: json_listings,
            count,
        })
    }

    async fn filters(
        &self,
        _ctx: Context<'_>,
        _req: EmptyRequest,
    ) -> Result<ListingFiltersResult, HandlerError> {
        let pool = self.deps.pool();

        let kinds = TagKindConfig::find_for_resource_type("listing", pool)
            .await
            .map_err(|e| TerminalError::new(format!("Query failed: {}", e)))?;

        let mut results = Vec::new();
        for kind in kinds {
            let counts = ListingStats::count_by_tag_kind_public(&kind.slug, pool)
                .await
                .unwrap_or_default();

            results.push(FilterKindResult {
                slug: kind.slug,
                display_name: kind.display_name,
                required: kind.required,
                values: counts,
            });
        }

        Ok(ListingFiltersResult { kinds: results })
    }

    async fn stats(
        &self,
        _ctx: Context<'_>,
        _req: EmptyRequest,
    ) -> Result<ListingStatsResult, HandlerError> {
        let pool = self.deps.pool();
        let stats = ListingStats::compute(pool)
            .await
            .map_err(|e| TerminalError::new(format!("Stats failed: {}", e)))?;
        Ok(ListingStatsResult { stats })
    }

    async fn score_batch(
        &self,
        _ctx: Context<'_>,
        req: ScoreBatchRequest,
    ) -> Result<ScoreBatchResult, HandlerError> {
        let pool = self.deps.pool();
        let mut scored = 0u32;

        for id_str in &req.listing_ids {
            let id: uuid::Uuid = id_str
                .parse()
                .map_err(|e: uuid::Error| TerminalError::new(format!("Invalid UUID: {}", e)))?;

            // Simple relevance scoring: count of tags as a proxy
            let tag_count = sqlx::query_as::<_, (i64,)>(
                "SELECT COUNT(*) FROM taggables WHERE taggable_type = 'listing' AND taggable_id = $1",
            )
            .bind(id)
            .fetch_one(pool)
            .await
            .map_err(|e| TerminalError::new(format!("Query: {}", e)))?;

            let score = (tag_count.0.min(10)) as i32;
            sqlx::query("UPDATE listings SET relevance_score = $1 WHERE id = $2")
                .bind(score)
                .bind(id)
                .execute(pool)
                .await
                .map_err(|e| TerminalError::new(format!("Update: {}", e)))?;

            scored += 1;
        }

        Ok(ScoreBatchResult { scored })
    }

    async fn expire_stale(
        &self,
        _ctx: Context<'_>,
        _req: EmptyRequest,
    ) -> Result<ExpireResult, HandlerError> {
        let pool = self.deps.pool();

        let result = sqlx::query(
            "UPDATE listings SET status = 'expired' WHERE expires_at < NOW() AND status = 'active'",
        )
        .execute(pool)
        .await
        .map_err(|e| TerminalError::new(format!("Expire: {}", e)))?;

        Ok(ExpireResult {
            expired: result.rows_affected() as u32,
        })
    }
}
