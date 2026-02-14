use chrono::{DateTime, NaiveDate, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::listings::ListingFilters;

/// Input parameters for the hybrid search engine.
#[derive(Debug, Clone, Default)]
pub struct HybridSearchParams {
    pub query_embedding: Option<pgvector::Vector>,
    pub query_text: Option<String>,
    pub filters: ListingFilters,
    pub temporal: TemporalFilter,
    pub locale: String,
    pub limit: i64,
    pub offset: i64,
}

/// A single search result row with scoring metadata.
/// Dedicated struct with explicit column selection -- never uses SELECT *.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct SearchResultRow {
    pub id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
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

/// Temporal filter for schedule-aware queries.
#[derive(Debug, Clone, Default)]
pub struct TemporalFilter {
    pub happening_on: Option<NaiveDate>,
    pub happening_between: Option<(NaiveDate, NaiveDate)>,
    pub day_of_week: Option<String>,
}

/// Parsed result from NLQ processing.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ParsedQuery {
    pub search_text: Option<String>,
    pub filters: ParsedFilters,
    pub temporal: Option<ParsedTemporal>,
    pub intent: SearchIntent,
    pub reasoning: String,
}

/// Taxonomy filters extracted from natural language.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ParsedFilters {
    pub signal_domain: Option<String>,
    pub audience_role: Option<String>,
    pub category: Option<String>,
    pub listing_type: Option<String>,
    pub urgency: Option<String>,
    pub capacity_status: Option<String>,
    pub radius_relevant: Option<String>,
    pub population: Option<String>,
}

/// Temporal intent extracted from natural language.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ParsedTemporal {
    pub happening_on: Option<String>,
    pub happening_between: Option<String>,
    pub day_of_week: Option<String>,
}

/// Classification of the user's query intent.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SearchIntent {
    InScope,
    OutOfScope,
    NeedsClarification,
    KnowledgeQuestion,
}

/// The search mode used to produce results.
#[derive(Debug, Clone, Serialize)]
pub enum SearchMode {
    SemanticPlusFts,
    FtsOnly,
    FiltersOnly,
}

impl SearchMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SemanticPlusFts => "semantic+fts",
            Self::FtsOnly => "fts_only",
            Self::FiltersOnly => "filters_only",
        }
    }
}

/// Container for search results with metadata.
#[derive(Debug, Clone)]
pub struct SearchResponse {
    pub results: Vec<SearchResultRow>,
    pub total_estimate: i64,
    pub mode: SearchMode,
    pub took_ms: u64,
}
