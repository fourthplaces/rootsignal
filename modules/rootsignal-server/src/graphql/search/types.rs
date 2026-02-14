use async_graphql::*;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::graphql::loaders::*;

use super::super::entities::types::GqlEntity;
use super::super::locations::types::GqlLocation;
use super::super::schedules::types::GqlSchedule;
use super::super::tags::types::GqlTag;

// ─── Search Result ──────────────────────────────────────────────────────────

#[derive(SimpleObject, Clone)]
#[graphql(complex)]
pub struct GqlSearchResult {
    pub id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    #[graphql(skip)]
    pub entity_id: Option<Uuid>,
    pub entity_name: Option<String>,
    pub entity_type: Option<String>,
    pub source_url: Option<String>,
    pub location_text: Option<String>,
    pub created_at: DateTime<Utc>,
    pub source_locale: String,
    pub locale: String,
    pub is_fallback: bool,
    pub semantic_score: Option<f64>,
    pub text_score: Option<f64>,
    pub combined_score: f64,
    pub distance_miles: Option<f64>,
}

impl From<rootsignal_domains::search::SearchResultRow> for GqlSearchResult {
    fn from(r: rootsignal_domains::search::SearchResultRow) -> Self {
        Self {
            id: r.id,
            title: r.title,
            description: r.description,
            status: r.status,
            entity_id: r.entity_id,
            entity_name: r.entity_name,
            entity_type: r.entity_type,
            source_url: r.source_url,
            location_text: r.location_text,
            created_at: r.created_at,
            source_locale: r.source_locale,
            locale: r.locale,
            is_fallback: r.is_fallback,
            semantic_score: r.semantic_score,
            text_score: r.text_score,
            combined_score: r.combined_score,
            distance_miles: r.distance_miles,
        }
    }
}

#[ComplexObject]
impl GqlSearchResult {
    async fn entity(&self, ctx: &Context<'_>) -> Result<Option<GqlEntity>> {
        let entity_id = match self.entity_id {
            Some(id) => id,
            None => return Ok(None),
        };

        let loader = ctx.data_unchecked::<async_graphql::dataloader::DataLoader<EntityByIdLoader>>();
        Ok(loader.load_one(entity_id).await?)
    }

    async fn tags(&self, ctx: &Context<'_>) -> Result<Vec<GqlTag>> {
        let loader = ctx.data_unchecked::<async_graphql::dataloader::DataLoader<TagsForLoader>>();
        let key = PolymorphicKey("listing".to_string(), self.id);
        Ok(loader.load_one(key).await?.unwrap_or_default())
    }

    async fn locations(&self, ctx: &Context<'_>) -> Result<Vec<GqlLocation>> {
        let loader =
            ctx.data_unchecked::<async_graphql::dataloader::DataLoader<LocationsForLoader>>();
        let key = PolymorphicKey("listing".to_string(), self.id);
        Ok(loader.load_one(key).await?.unwrap_or_default())
    }

    async fn schedules(&self, ctx: &Context<'_>) -> Result<Vec<GqlSchedule>> {
        let loader =
            ctx.data_unchecked::<async_graphql::dataloader::DataLoader<SchedulesForLoader>>();
        let key = PolymorphicKey("listing".to_string(), self.id);
        Ok(loader.load_one(key).await?.unwrap_or_default())
    }
}

// ─── Search Response ────────────────────────────────────────────────────────

#[derive(SimpleObject)]
pub struct GqlSearchResponse {
    pub results: Vec<GqlSearchResult>,
    pub total_estimate: i32,
    pub mode: String,
    pub took_ms: i32,
}

impl From<rootsignal_domains::search::SearchResponse> for GqlSearchResponse {
    fn from(r: rootsignal_domains::search::SearchResponse) -> Self {
        Self {
            results: r.results.into_iter().map(GqlSearchResult::from).collect(),
            total_estimate: r.total_estimate as i32,
            mode: r.mode.as_str().to_string(),
            took_ms: r.took_ms as i32,
        }
    }
}

// ─── NLQ Types ──────────────────────────────────────────────────────────────

#[derive(SimpleObject)]
pub struct GqlParsedQuery {
    pub search_text: Option<String>,
    pub filters: GqlParsedFilters,
    pub temporal: Option<GqlParsedTemporal>,
    pub intent: GqlSearchIntent,
    pub reasoning: String,
}

impl From<rootsignal_domains::search::ParsedQuery> for GqlParsedQuery {
    fn from(p: rootsignal_domains::search::ParsedQuery) -> Self {
        Self {
            search_text: p.search_text,
            filters: GqlParsedFilters::from(p.filters),
            temporal: p.temporal.map(GqlParsedTemporal::from),
            intent: GqlSearchIntent::from(p.intent),
            reasoning: p.reasoning,
        }
    }
}

#[derive(SimpleObject)]
pub struct GqlParsedFilters {
    pub signal_domain: Option<String>,
    pub audience_role: Option<String>,
    pub category: Option<String>,
    pub listing_type: Option<String>,
    pub urgency: Option<String>,
    pub capacity_status: Option<String>,
    pub radius_relevant: Option<String>,
    pub population: Option<String>,
}

impl From<rootsignal_domains::search::ParsedFilters> for GqlParsedFilters {
    fn from(f: rootsignal_domains::search::ParsedFilters) -> Self {
        Self {
            signal_domain: f.signal_domain,
            audience_role: f.audience_role,
            category: f.category,
            listing_type: f.listing_type,
            urgency: f.urgency,
            capacity_status: f.capacity_status,
            radius_relevant: f.radius_relevant,
            population: f.population,
        }
    }
}

#[derive(SimpleObject)]
pub struct GqlParsedTemporal {
    pub happening_on: Option<String>,
    pub happening_between: Option<String>,
    pub day_of_week: Option<String>,
}

impl From<rootsignal_domains::search::ParsedTemporal> for GqlParsedTemporal {
    fn from(t: rootsignal_domains::search::ParsedTemporal) -> Self {
        Self {
            happening_on: t.happening_on,
            happening_between: t.happening_between,
            day_of_week: t.day_of_week,
        }
    }
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum GqlSearchIntent {
    InScope,
    OutOfScope,
    NeedsClarification,
    KnowledgeQuestion,
}

impl From<rootsignal_domains::search::SearchIntent> for GqlSearchIntent {
    fn from(i: rootsignal_domains::search::SearchIntent) -> Self {
        match i {
            rootsignal_domains::search::SearchIntent::InScope => Self::InScope,
            rootsignal_domains::search::SearchIntent::OutOfScope => Self::OutOfScope,
            rootsignal_domains::search::SearchIntent::NeedsClarification => Self::NeedsClarification,
            rootsignal_domains::search::SearchIntent::KnowledgeQuestion => Self::KnowledgeQuestion,
        }
    }
}

// ─── NLQ Search Response ────────────────────────────────────────────────────

#[derive(SimpleObject)]
pub struct GqlNlqSearchResponse {
    pub parsed: GqlParsedQuery,
    pub results: Option<GqlSearchResponse>,
}
