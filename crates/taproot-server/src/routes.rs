use async_graphql::http::GraphiQLSource;
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    response::{Html, IntoResponse},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use sqlx::PgPool;
use serde::Serialize;
use std::sync::Arc;
use taproot_core::ServerDeps;
use taproot_domains::heat_map::HeatMapPoint;
use taproot_domains::listings::{ListingDetail, ListingFilters, ListingStats, ListingWithDistance};
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

use crate::graphql::{self, AppSchema};
use crate::graphql::auth;
use crate::graphql::context;

pub fn build_router(deps: Arc<ServerDeps>, allowed_origins: &[String]) -> Router {
    let pool = deps.pool().clone();

    // Build JWT service for cookie extraction in request handler
    let jwt_service = deps
        .config
        .jwt_secret
        .as_ref()
        .map(|secret| auth::jwt::JwtService::new(secret, "taproot".to_string()));

    let schema = graphql::build_schema(deps);

    let cors = if allowed_origins.is_empty() {
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any)
    } else {
        let origins: Vec<_> = allowed_origins
            .iter()
            .filter_map(|o| o.parse().ok())
            .collect();
        CorsLayer::new()
            .allow_origin(origins)
            .allow_methods(Any)
            .allow_headers(Any)
    };

    Router::new()
        .route("/", get(assessment_page))
        .route("/graphql", get(graphiql_handler).post(graphql_handler))
        // REST API — deprecated, use GraphQL at /graphql instead.
        // These routes will be removed in a future release.
        .route("/api/stats", get(api_stats))
        .route("/api/listings", get(api_listings))
        .route("/api/listings/:id/cluster", get(api_listing_cluster))
        .route("/api/heatmap", get(api_heatmap))
        .route("/health", get(health))
        .layer(cors)
        .with_state(AppState { pool, schema, jwt_service })
}

#[derive(Clone)]
pub struct AppState {
    pool: PgPool,
    schema: AppSchema,
    jwt_service: Option<auth::jwt::JwtService>,
}

async fn graphql_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    req: GraphQLRequest,
) -> GraphQLResponse {
    let locale = context::extract_locale(&headers, None);

    // Extract auth claims from cookie if JWT is configured
    let claims: Option<auth::jwt::Claims> = state.jwt_service.as_ref().and_then(|jwt| {
        let cookie = headers.get("cookie").and_then(|v| v.to_str().ok());
        auth::middleware::extract_claims(jwt, cookie)
    });

    let request = req.into_inner().data(locale).data(claims);
    let span = tracing::info_span!("graphql_request");
    let _enter = span.enter();
    let response = state.schema.execute(request).await;
    if !response.errors.is_empty() {
        tracing::warn!(errors = ?response.errors, "GraphQL errors");
    }
    response.into()
}

async fn graphiql_handler() -> impl IntoResponse {
    Html(GraphiQLSource::build().endpoint("/graphql").finish())
}

async fn health() -> &'static str {
    "ok"
}

async fn api_stats(State(state): State<AppState>) -> Json<ListingStats> {
    let stats = ListingStats::compute(&state.pool).await.unwrap_or_else(|_| ListingStats {
        total_listings: 0,
        active_listings: 0,
        total_sources: 0,
        total_snapshots: 0,
        total_extractions: 0,
        total_entities: 0,
        listings_by_type: vec![],
        listings_by_role: vec![],
        listings_by_category: vec![],
        listings_by_domain: vec![],
        listings_by_urgency: vec![],
        listings_by_confidence: vec![],
        listings_by_capacity: vec![],
        recent_7d: 0,
    });
    Json(stats)
}

#[derive(Deserialize)]
struct ListingsQuery {
    limit: Option<i64>,
    offset: Option<i64>,
    locale: Option<String>,
    zip_code: Option<String>,
    radius_miles: Option<f64>,
}

/// Parse Accept-Language header to extract the primary locale.
fn parse_accept_language(headers: &HeaderMap) -> Option<String> {
    let header = headers.get("accept-language")?.to_str().ok()?;
    // Take the first language tag (highest priority)
    let primary = header.split(',').next()?.trim();
    // Strip quality factor if present (e.g., "es;q=0.9" → "es")
    let lang = primary.split(';').next()?.trim();
    // Only accept our supported locales
    match lang {
        "en" | "es" | "so" | "ht" => Some(lang.to_string()),
        _ => None,
    }
}

/// Response enum to support both regular listings and distance-enriched listings.
#[derive(Serialize)]
#[serde(untagged)]
enum ListingsResponse {
    Standard(Vec<ListingDetail>),
    WithDistance(Vec<ListingWithDistance>),
}

async fn api_listings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<ListingsQuery>,
) -> Json<ListingsResponse> {
    let limit = params.limit.unwrap_or(50);
    let offset = params.offset.unwrap_or(0);

    let locale = params
        .locale
        .or_else(|| parse_accept_language(&headers))
        .unwrap_or_else(|| "en".to_string());

    if let Some(zip) = &params.zip_code {
        let radius = params.radius_miles.unwrap_or(25.0).min(100.0);
        let filters = ListingFilters::default();
        let listings = ListingWithDistance::find_near_zip(
            zip, radius, &filters, limit, offset, &locale, &state.pool,
        )
        .await
        .unwrap_or_default();
        Json(ListingsResponse::WithDistance(listings))
    } else {
        let listings = ListingDetail::find_active_localized(limit, offset, &locale, &state.pool)
            .await
            .unwrap_or_default();
        Json(ListingsResponse::Standard(listings))
    }
}

async fn api_listing_cluster(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Json<Vec<ListingDetail>> {
    let siblings = ListingDetail::cluster_siblings(id, &state.pool)
        .await
        .unwrap_or_default();
    Json(siblings)
}

#[derive(Deserialize)]
struct HeatmapQuery {
    zip_code: Option<String>,
    radius_miles: Option<f64>,
    entity_type: Option<String>,
}

async fn api_heatmap(
    State(state): State<AppState>,
    Query(params): Query<HeatmapQuery>,
) -> Json<Vec<HeatMapPoint>> {
    let points = if let Some(zip) = &params.zip_code {
        let radius = params.radius_miles.unwrap_or(25.0).min(100.0);
        HeatMapPoint::find_near_zip(zip, radius, &state.pool)
            .await
            .unwrap_or_default()
    } else if let Some(entity_type) = &params.entity_type {
        HeatMapPoint::find_latest_by_type(entity_type, &state.pool)
            .await
            .unwrap_or_default()
    } else {
        HeatMapPoint::find_latest(&state.pool)
            .await
            .unwrap_or_default()
    };
    Json(points)
}

async fn assessment_page(State(state): State<AppState>) -> Html<String> {
    let stats = ListingStats::compute(&state.pool).await.unwrap_or_else(|_| ListingStats {
        total_listings: 0,
        active_listings: 0,
        total_sources: 0,
        total_snapshots: 0,
        total_extractions: 0,
        total_entities: 0,
        listings_by_type: vec![],
        listings_by_role: vec![],
        listings_by_category: vec![],
        listings_by_domain: vec![],
        listings_by_urgency: vec![],
        listings_by_confidence: vec![],
        listings_by_capacity: vec![],
        recent_7d: 0,
    });

    let listings = ListingDetail::find_active(30, 0, &state.pool)
        .await
        .unwrap_or_default();

    let type_rows: String = stats
        .listings_by_type
        .iter()
        .map(|t| format!("<tr><td>{}</td><td>{}</td></tr>", t.value, t.count))
        .collect::<Vec<_>>()
        .join("\n");

    let role_rows: String = stats
        .listings_by_role
        .iter()
        .map(|t| format!("<tr><td>{}</td><td>{}</td></tr>", t.value, t.count))
        .collect::<Vec<_>>()
        .join("\n");

    let category_rows: String = stats
        .listings_by_category
        .iter()
        .map(|t| format!("<tr><td>{}</td><td>{}</td></tr>", t.value, t.count))
        .collect::<Vec<_>>()
        .join("\n");

    let domain_rows: String = stats
        .listings_by_domain
        .iter()
        .map(|t| format!("<tr><td>{}</td><td>{}</td></tr>", t.value, t.count))
        .collect::<Vec<_>>()
        .join("\n");

    let urgency_rows: String = stats
        .listings_by_urgency
        .iter()
        .map(|t| format!("<tr><td>{}</td><td>{}</td></tr>", t.value, t.count))
        .collect::<Vec<_>>()
        .join("\n");

    let confidence_rows: String = stats
        .listings_by_confidence
        .iter()
        .map(|t| format!("<tr><td>{}</td><td>{}</td></tr>", t.value, t.count))
        .collect::<Vec<_>>()
        .join("\n");

    let capacity_rows: String = stats
        .listings_by_capacity
        .iter()
        .map(|t| format!("<tr><td>{}</td><td>{}</td></tr>", t.value, t.count))
        .collect::<Vec<_>>()
        .join("\n");

    let listing_rows: String = listings
        .iter()
        .map(|l| {
            let timing = l
                .schedule_description
                .as_deref()
                .unwrap_or("-")
                .to_string();
            let source = l
                .source_url
                .as_deref()
                .map(|u| format!("<a href=\"{}\" target=\"_blank\">link</a>", u))
                .unwrap_or_else(|| "-".to_string());
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                l.title,
                l.entity_name.as_deref().unwrap_or("-"),
                l.entity_type.as_deref().unwrap_or("-"),
                l.location_text.as_deref().unwrap_or("-"),
                timing,
                source,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <title>Taproot - Signal Assessment</title>
    <style>
        body {{ font-family: system-ui, -apple-system, sans-serif; max-width: 1200px; margin: 0 auto; padding: 20px; background: #f5f5f5; }}
        h1 {{ color: #2d5016; }}
        h2 {{ color: #444; margin-top: 30px; }}
        .stats {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 16px; margin: 20px 0; }}
        .stat {{ background: white; padding: 20px; border-radius: 8px; box-shadow: 0 1px 3px rgba(0,0,0,0.1); }}
        .stat .value {{ font-size: 2em; font-weight: bold; color: #2d5016; }}
        .stat .label {{ color: #666; font-size: 0.9em; }}
        table {{ width: 100%; border-collapse: collapse; background: white; border-radius: 8px; overflow: hidden; box-shadow: 0 1px 3px rgba(0,0,0,0.1); }}
        th, td {{ padding: 10px 14px; text-align: left; border-bottom: 1px solid #eee; }}
        th {{ background: #2d5016; color: white; }}
        tr:hover {{ background: #f9f9f9; }}
        .gate {{ padding: 8px 16px; border-radius: 4px; margin: 4px 0; }}
        .gate.pass {{ background: #d4edda; color: #155724; }}
        .gate.fail {{ background: #f8d7da; color: #721c24; }}
    </style>
</head>
<body>
    <h1>Taproot Signal Assessment</h1>
    <p>Milestone 1: Signal Proof</p>

    <div class="stats">
        <div class="stat"><div class="value">{active}</div><div class="label">Active Listings</div></div>
        <div class="stat"><div class="value">{sources}</div><div class="label">Sources</div></div>
        <div class="stat"><div class="value">{snapshots}</div><div class="label">Page Snapshots</div></div>
        <div class="stat"><div class="value">{extractions}</div><div class="label">Extractions</div></div>
        <div class="stat"><div class="value">{entities}</div><div class="label">Entities</div></div>
        <div class="stat"><div class="value">{recent_7d}</div><div class="label">Fresh (7 days)</div></div>
    </div>

    <h2>Milestone Gates</h2>
    <div class="{gate_vol}">Volume: {active} active listings (target: 100+)</div>
    <div class="{gate_fresh}">Freshness: {recent_7d} listings with timing in last 7 days</div>
    <div class="{gate_types}">Type diversity: {type_count} listing types (target: 3+)</div>
    <div class="{gate_roles}">Role diversity: {role_count} audience roles (target: 3+)</div>

    <h2>By Listing Type</h2>
    <table><tr><th>Type</th><th>Count</th></tr>{type_rows}</table>

    <h2>By Audience Role</h2>
    <table><tr><th>Role</th><th>Count</th></tr>{role_rows}</table>

    <h2>By Category</h2>
    <table><tr><th>Category</th><th>Count</th></tr>{category_rows}</table>

    <h2>By Signal Domain</h2>
    <table><tr><th>Domain</th><th>Count</th></tr>{domain_rows}</table>

    <h2>By Urgency</h2>
    <table><tr><th>Urgency</th><th>Count</th></tr>{urgency_rows}</table>

    <h2>By Confidence</h2>
    <table><tr><th>Confidence</th><th>Count</th></tr>{confidence_rows}</table>

    <h2>By Capacity Status</h2>
    <table><tr><th>Status</th><th>Count</th></tr>{capacity_rows}</table>

    <h2>Sample Listings (30 random)</h2>
    <table>
        <tr><th>Title</th><th>Entity</th><th>Type</th><th>Location</th><th>Timing</th><th>Source</th></tr>
        {listing_rows}
    </table>
</body>
</html>"#,
        active = stats.active_listings,
        sources = stats.total_sources,
        snapshots = stats.total_snapshots,
        extractions = stats.total_extractions,
        entities = stats.total_entities,
        recent_7d = stats.recent_7d,
        gate_vol = if stats.active_listings >= 100 { "gate pass" } else { "gate fail" },
        gate_fresh = if stats.recent_7d > 0 { "gate pass" } else { "gate fail" },
        type_count = stats.listings_by_type.len(),
        gate_types = if stats.listings_by_type.len() >= 3 { "gate pass" } else { "gate fail" },
        role_count = stats.listings_by_role.len(),
        gate_roles = if stats.listings_by_role.len() >= 3 { "gate pass" } else { "gate fail" },
        type_rows = type_rows,
        role_rows = role_rows,
        category_rows = category_rows,
        domain_rows = domain_rows,
        urgency_rows = urgency_rows,
        confidence_rows = confidence_rows,
        capacity_rows = capacity_rows,
        listing_rows = listing_rows,
    );

    Html(html)
}
