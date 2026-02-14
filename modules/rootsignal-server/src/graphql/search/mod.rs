pub mod types;

use std::sync::Arc;

use async_graphql::*;
use chrono::NaiveDate;
use rootsignal_core::ServerDeps;
use rootsignal_domains::listings::ListingFilters;
use rootsignal_domains::search;

use crate::graphql::context::Locale;
use crate::graphql::error;
use types::*;

#[derive(Default)]
pub struct SearchQuery;

#[Object]
impl SearchQuery {
    /// Hybrid search: combines semantic similarity (pgvector) with full-text search (tsvector).
    /// When `q` is provided, embeds the query and runs RRF-ranked hybrid search.
    /// When `q` is omitted, filters and sorts by relevance_score.
    async fn search(
        &self,
        ctx: &Context<'_>,
        q: Option<String>,
        // Tag filters
        signal_domain: Option<String>,
        audience_role: Option<String>,
        category: Option<String>,
        listing_type: Option<String>,
        urgency: Option<String>,
        confidence: Option<String>,
        capacity_status: Option<String>,
        radius_relevant: Option<String>,
        population: Option<String>,
        // Geo
        zip_code: Option<String>,
        radius_miles: Option<f64>,
        lat: Option<f64>,
        lng: Option<f64>,
        radius_km: Option<f64>,
        // Temporal
        happening_on: Option<String>,
        happening_between: Option<String>,
        day_of_week: Option<String>,
        // Pagination
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> Result<GqlSearchResponse> {
        let deps = ctx.data_unchecked::<Arc<ServerDeps>>();
        let locale = ctx.data_unchecked::<Locale>();
        let pool = deps.pool();

        let limit = limit.unwrap_or(20).min(100) as i64;
        let offset = offset.unwrap_or(0).max(0) as i64;

        // Resolve zip_code to lat/lng for geo filtering
        let (resolved_lat, resolved_lng, resolved_radius_km) =
            resolve_geo(lat, lng, radius_km, zip_code.as_deref(), radius_miles, pool).await;

        // Translate non-English query to English
        let english_q = if let Some(ref query) = q {
            if locale.0 != "en" {
                match search::translate_query_to_english(query, &locale.0, deps).await {
                    Ok(translated) => Some(translated),
                    Err(_) => Some(query.clone()),
                }
            } else {
                Some(query.clone())
            }
        } else {
            None
        };

        // Embed the query for semantic search
        let query_embedding = if let Some(ref eq) = english_q {
            match deps.embedding_service.embed(eq).await {
                Ok(vec) => Some(pgvector::Vector::from(vec)),
                Err(e) => {
                    tracing::warn!(error = %e, "Embedding failed, falling back to FTS-only");
                    None
                }
            }
        } else {
            None
        };

        let params = search::HybridSearchParams {
            query_embedding,
            query_text: english_q,
            filters: ListingFilters {
                signal_domain,
                audience_role,
                category,
                listing_type,
                urgency,
                confidence,
                capacity_status,
                radius_relevant,
                population,
                lat: resolved_lat,
                lng: resolved_lng,
                radius_km: resolved_radius_km,
                ..Default::default()
            },
            temporal: parse_temporal(happening_on, happening_between, day_of_week)?,
            locale: locale.0.clone(),
            limit,
            offset,
        };

        let response = search::hybrid_search(&params, pool)
            .await
            .map_err(|e| error::internal(e))?;

        Ok(GqlSearchResponse::from(response))
    }

    /// Parse a natural language query into structured filters + search text.
    /// Optionally auto-executes the search when `auto_search` is true.
    async fn parse_query(
        &self,
        ctx: &Context<'_>,
        q: String,
        auto_search: Option<bool>,
    ) -> Result<GqlNlqSearchResponse> {
        let deps = ctx.data_unchecked::<Arc<ServerDeps>>();
        let locale = ctx.data_unchecked::<Locale>();
        let pool = deps.pool();

        // Translate if non-English
        let english_q = if locale.0 != "en" {
            search::translate_query_to_english(&q, &locale.0, deps)
                .await
                .unwrap_or_else(|_| q.clone())
        } else {
            q.clone()
        };

        let parsed = search::parse_natural_language_query(&english_q, deps)
            .await
            .map_err(|e| error::internal(e))?;

        let results = if auto_search.unwrap_or(false) {
            // Convert parsed query into search params
            let query_text = parsed.search_text.clone();

            let query_embedding = if let Some(ref qt) = query_text {
                match deps.embedding_service.embed(qt).await {
                    Ok(vec) => Some(pgvector::Vector::from(vec)),
                    Err(_) => None,
                }
            } else {
                None
            };

            let temporal = if let Some(ref pt) = parsed.temporal {
                parse_temporal(
                    pt.happening_on.clone(),
                    pt.happening_between.clone(),
                    pt.day_of_week.clone(),
                )?
            } else {
                search::TemporalFilter::default()
            };

            let params = search::HybridSearchParams {
                query_embedding,
                query_text,
                filters: ListingFilters {
                    signal_domain: parsed.filters.signal_domain.clone(),
                    audience_role: parsed.filters.audience_role.clone(),
                    category: parsed.filters.category.clone(),
                    listing_type: parsed.filters.listing_type.clone(),
                    urgency: parsed.filters.urgency.clone(),
                    capacity_status: parsed.filters.capacity_status.clone(),
                    radius_relevant: parsed.filters.radius_relevant.clone(),
                    population: parsed.filters.population.clone(),
                    ..Default::default()
                },
                temporal,
                locale: locale.0.clone(),
                limit: 20,
                offset: 0,
            };

            let response = search::hybrid_search(&params, pool)
                .await
                .map_err(|e| error::internal(e))?;

            Some(GqlSearchResponse::from(response))
        } else {
            None
        };

        Ok(GqlNlqSearchResponse {
            parsed: GqlParsedQuery::from(parsed),
            results,
        })
    }
}

/// Parse temporal filter strings into a TemporalFilter.
fn parse_temporal(
    happening_on: Option<String>,
    happening_between: Option<String>,
    day_of_week: Option<String>,
) -> Result<search::TemporalFilter> {
    let on = happening_on
        .as_deref()
        .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
        .transpose()
        .map_err(|e| Error::new(format!("Invalid happening_on date: {}", e)))?;

    let between = happening_between
        .as_deref()
        .map(|s| {
            let parts: Vec<&str> = s.split('/').collect();
            if parts.len() != 2 {
                return Err(Error::new("happening_between must be 'YYYY-MM-DD/YYYY-MM-DD'"));
            }
            let start = NaiveDate::parse_from_str(parts[0], "%Y-%m-%d")
                .map_err(|e| Error::new(format!("Invalid start date: {}", e)))?;
            let end = NaiveDate::parse_from_str(parts[1], "%Y-%m-%d")
                .map_err(|e| Error::new(format!("Invalid end date: {}", e)))?;
            Ok((start, end))
        })
        .transpose()?;

    Ok(search::TemporalFilter {
        happening_on: on,
        happening_between: between,
        day_of_week,
    })
}

/// Resolve geo parameters: prefer explicit lat/lng, fall back to zip_code lookup.
async fn resolve_geo(
    lat: Option<f64>,
    lng: Option<f64>,
    radius_km: Option<f64>,
    zip_code: Option<&str>,
    radius_miles: Option<f64>,
    pool: &sqlx::PgPool,
) -> (Option<f64>, Option<f64>, Option<f64>) {
    if let (Some(lat), Some(lng)) = (lat, lng) {
        return (Some(lat), Some(lng), radius_km.or(Some(25.0)));
    }

    if let Some(zip) = zip_code {
        let result = sqlx::query_as::<_, (f64, f64)>(
            "SELECT latitude, longitude FROM zip_codes WHERE zip_code = $1",
        )
        .bind(zip)
        .fetch_optional(pool)
        .await;

        if let Ok(Some((lat, lng))) = result {
            let km = radius_miles.unwrap_or(25.0) * 1.60934;
            return (Some(lat), Some(lng), Some(km));
        }
    }

    (None, None, None)
}
