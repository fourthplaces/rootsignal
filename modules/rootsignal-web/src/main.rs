use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use axum::{
    extract::{ConnectInfo, Path, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use tokio::sync::Mutex;
use tower_http::set_header::SetResponseHeaderLayer;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

use rootsignal_common::{
    Config, DiscoveryMethod, EvidenceNode, Node, NodeType, SourceNode, SourceType, SubmissionNode,
};
use rootsignal_graph::{GraphClient, GraphWriter, PublicGraphReader};

mod templates;
use templates::*;

// --- App State ---

struct AppState {
    reader: PublicGraphReader,
    writer: GraphWriter,
    admin_username: String,
    admin_password: String,
    city: String,
    rate_limiter: Mutex<HashMap<IpAddr, Vec<Instant>>>,
}

// --- Main ---

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("rootsignal=info".parse()?))
        .init();

    let config = Config::web_from_env();

    let client =
        GraphClient::connect(&config.neo4j_uri, &config.neo4j_user, &config.neo4j_password)
            .await?;

    let state = Arc::new(AppState {
        reader: PublicGraphReader::new(client.clone()),
        writer: GraphWriter::new(client),
        admin_username: config.admin_username,
        admin_password: config.admin_password,
        city: config.city.clone(),
        rate_limiter: Mutex::new(HashMap::new()),
    });

    let app = Router::new()
        // Public routes
        .route("/", get(map_page))
        .route("/nodes", get(nodes_page))
        .route("/nodes/{id}", get(node_detail_page))
        .route("/api/nodes/near", get(api_nodes_near))
        // Stories API
        .route("/api/stories", get(api_stories))
        .route("/api/stories/{id}", get(api_story_detail))
        .route("/api/stories/{id}/signals", get(api_story_signals))
        .route("/api/stories/{id}/actors", get(api_story_actors))
        .route("/api/stories/category/{category}", get(api_stories_by_category))
        .route("/api/stories/arc/{arc}", get(api_stories_by_arc))
        .route("/api/signals", get(api_signals))
        .route("/api/signals/{id}", get(api_signal_detail))
        // Actors API
        .route("/api/actors", get(api_actors))
        .route("/api/actors/{id}", get(api_actor_detail))
        .route("/api/actors/{id}/stories", get(api_actor_stories))
        // Tension responses
        .route("/api/tensions/{id}/responses", get(api_tension_responses))
        // Editions API
        .route("/api/editions", get(api_editions))
        .route("/api/editions/latest", get(api_edition_latest))
        .route("/api/editions/{id}", get(api_edition_detail))
        // Submission endpoint
        .route("/api/submit", post(api_submit))
        // Admin route (basic auth checked in handler)
        .route("/admin/quality", get(quality_dashboard))
        .with_state(state)
        // Privacy headers: no caching, no tracking
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::PRAGMA,
            HeaderValue::from_static("no-cache"),
        ))
        // Logging layer: method + path + status + latency only (no query params, no IP)
        .layer(
            tower_http::trace::TraceLayer::new_for_http()
                .make_span_with(|request: &axum::http::Request<_>| {
                    // Only log the path — strip query params to avoid logging lat/lng coordinates
                    tracing::info_span!(
                        "http_request",
                        method = %request.method(),
                        path = %request.uri().path(),
                    )
                }),
        );

    let addr = format!("{}:{}", config.web_host, config.web_port);
    info!("Root Signal web server starting on {addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;

    Ok(())
}

// --- Handlers ---

async fn map_page() -> impl IntoResponse {
    Html(render_map())
}

async fn nodes_page(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.reader.list_recent(100, None).await {
        Ok(nodes) => {
            let view_nodes: Vec<NodeView> = nodes.iter().map(node_to_view).collect();
            Html(render_nodes(&view_nodes))
        }
        Err(e) => {
            warn!(error = %e, "Failed to load nodes");
            Html("<h1>Error loading signals</h1>".to_string())
        }
    }
}

async fn node_detail_page(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let uuid = match Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return (StatusCode::BAD_REQUEST, Html("Invalid ID".to_string())),
    };

    match state.reader.get_node_detail(uuid).await {
        Ok(Some((node, evidence))) => {
            let view = node_to_view(&node);
            let ev_views: Vec<EvidenceView> = evidence.iter().map(evidence_to_view).collect();
            (StatusCode::OK, Html(render_node_detail(&view, &ev_views)))
        }
        Ok(None) => (StatusCode::NOT_FOUND, Html("Signal not found".to_string())),
        Err(e) => {
            warn!(error = %e, "Failed to load node detail");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html("Error loading signal".to_string()),
            )
        }
    }
}

#[derive(Deserialize)]
struct NearQuery {
    lat: f64,
    lng: f64,
    radius: Option<f64>,
    types: Option<String>,
}

async fn api_nodes_near(
    State(state): State<Arc<AppState>>,
    Query(params): Query<NearQuery>,
) -> impl IntoResponse {
    let radius = params.radius.unwrap_or(10.0).min(50.0);
    let node_types: Option<Vec<NodeType>> = params.types.as_ref().map(|t| {
        t.split(',')
            .filter_map(|s| match s.trim() {
                "Event" | "event" => Some(NodeType::Event),
                "Give" | "give" => Some(NodeType::Give),
                "Ask" | "ask" => Some(NodeType::Ask),
                "Notice" | "notice" => Some(NodeType::Notice),
                "Tension" | "tension" => Some(NodeType::Tension),
                _ => None,
            })
            .collect()
    });

    match state
        .reader
        .find_nodes_near(params.lat, params.lng, radius, node_types.as_deref())
        .await
    {
        Ok(nodes) => {
            let geojson = nodes_to_geojson(&nodes);
            Json(geojson).into_response()
        }
        Err(e) => {
            warn!(error = %e, "Failed to load nodes near");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

// --- Stories & Signals API ---

#[derive(Deserialize)]
struct StoriesQuery {
    limit: Option<u32>,
    status: Option<String>,
}

async fn api_stories(
    State(state): State<Arc<AppState>>,
    Query(params): Query<StoriesQuery>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(20).min(100);
    match state
        .reader
        .top_stories_by_energy(limit, params.status.as_deref())
        .await
    {
        Ok(stories) => {
            let story_ids: Vec<Uuid> = stories.iter().map(|s| s.id).collect();
            let ev_counts = state
                .reader
                .story_evidence_counts(&story_ids)
                .await
                .unwrap_or_default();
            let stories_json: Vec<serde_json::Value> = stories
                .iter()
                .map(|s| {
                    let ec = ev_counts
                        .iter()
                        .find(|(id, _)| *id == s.id)
                        .map(|(_, c)| *c)
                        .unwrap_or(0);
                    let mut val = serde_json::to_value(s).unwrap_or_default();
                    if let Some(obj) = val.as_object_mut() {
                        obj.insert("evidence_count".to_string(), serde_json::json!(ec));
                    }
                    val
                })
                .collect();
            Json(serde_json::json!({ "stories": stories_json })).into_response()
        }
        Err(e) => {
            warn!(error = %e, "Failed to load stories");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn api_story_detail(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let uuid = match Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };

    match state.reader.get_story_with_signals(uuid).await {
        Ok(Some((story, signals))) => {
            let all_evidence = state
                .reader
                .get_story_signal_evidence(uuid)
                .await
                .unwrap_or_default();
            let tension_responses = state
                .reader
                .get_story_tension_responses(uuid)
                .await
                .unwrap_or_default();
            let signal_views: Vec<serde_json::Value> = signals
                .iter()
                .filter_map(|n| {
                    let meta = n.meta()?;
                    let evidence: &Vec<EvidenceNode> = &all_evidence
                        .iter()
                        .find(|(id, _)| *id == meta.id)
                        .map(|(_, ev)| ev.clone())
                        .unwrap_or_default();
                    let ev_json: Vec<serde_json::Value> = evidence
                        .iter()
                        .map(|e| {
                            serde_json::json!({
                                "source_url": e.source_url,
                                "snippet": e.snippet,
                                "relevance": e.relevance,
                                "evidence_confidence": e.evidence_confidence,
                            })
                        })
                        .collect();
                    let mut signal_json = serde_json::json!({
                        "id": meta.id.to_string(),
                        "title": meta.title,
                        "summary": meta.summary,
                        "node_type": format!("{}", n.node_type()),
                        "confidence": meta.confidence,
                        "source_url": meta.source_url,
                        "evidence_count": evidence.len(),
                        "evidence": ev_json,
                    });
                    // For Tension signals, include type-specific fields and responses
                    if let Node::Tension(t) = n {
                        if let Some(obj) = signal_json.as_object_mut() {
                            obj.insert("severity".into(), serde_json::json!(format!("{:?}", t.severity)));
                            obj.insert("category".into(), serde_json::json!(t.category));
                            obj.insert("what_would_help".into(), serde_json::json!(t.what_would_help));
                        }
                        let responses: Vec<&serde_json::Value> = tension_responses
                            .iter()
                            .filter(|(tid, _)| *tid == meta.id)
                            .flat_map(|(_, resps)| resps.iter())
                            .collect();
                        if let Some(obj) = signal_json.as_object_mut() {
                            obj.insert("responses".to_string(), serde_json::json!(responses));
                            obj.insert("response_count".to_string(), serde_json::json!(responses.len()));
                        }
                    }
                    Some(signal_json)
                })
                .collect();
            Json(serde_json::json!({
                "story": story,
                "signals": signal_views,
            }))
            .into_response()
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            warn!(error = %e, "Failed to load story detail");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn api_story_signals(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let uuid = match Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };

    match state.reader.get_story_signals(uuid).await {
        Ok(signals) => {
            let geojson = nodes_to_geojson(&signals);
            Json(geojson).into_response()
        }
        Err(e) => {
            warn!(error = %e, "Failed to load story signals");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[derive(Deserialize)]
struct SignalsQuery {
    limit: Option<u32>,
    types: Option<String>,
}

async fn api_signals(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SignalsQuery>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(50).min(200);
    let node_types: Option<Vec<NodeType>> = params.types.as_ref().map(|t| {
        t.split(',')
            .filter_map(|s| match s.trim() {
                "Event" | "event" => Some(NodeType::Event),
                "Give" | "give" => Some(NodeType::Give),
                "Ask" | "ask" => Some(NodeType::Ask),
                "Notice" | "notice" => Some(NodeType::Notice),
                "Tension" | "tension" => Some(NodeType::Tension),
                _ => None,
            })
            .collect()
    });

    match state
        .reader
        .list_recent(limit, node_types.as_deref())
        .await
    {
        Ok(nodes) => {
            let geojson = nodes_to_geojson(&nodes);
            Json(geojson).into_response()
        }
        Err(e) => {
            warn!(error = %e, "Failed to load signals");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn api_signal_detail(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let uuid = match Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };

    match state.reader.get_node_detail(uuid).await {
        Ok(Some((node, evidence))) => {
            let meta = node.meta();
            let ev_views: Vec<serde_json::Value> = evidence
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "source_url": e.source_url,
                        "snippet": e.snippet,
                        "relevance": e.relevance,
                        "evidence_confidence": e.evidence_confidence,
                        "retrieved_at": e.retrieved_at.to_rfc3339(),
                        "content_hash": e.content_hash,
                    })
                })
                .collect();

            let action_url = match &node {
                Node::Event(e) => Some(e.action_url.clone()),
                Node::Give(g) => Some(g.action_url.clone()),
                Node::Ask(a) => a.action_url.clone(),
                _ => None,
            };

            let mut signal_json = serde_json::json!({
                "id": meta.map(|m| m.id.to_string()),
                "title": meta.map(|m| &m.title),
                "summary": meta.map(|m| &m.summary),
                "node_type": format!("{}", node.node_type()),
                "confidence": meta.map(|m| m.confidence),
                "corroboration_count": meta.map(|m| m.corroboration_count),
                "source_diversity": meta.map(|m| m.source_diversity),
                "external_ratio": meta.map(|m| m.external_ratio),
                "cause_heat": meta.map(|m| m.cause_heat),
                "source_url": meta.map(|m| &m.source_url),
                "action_url": action_url,
                "location": meta.and_then(|m| m.location).map(|l| serde_json::json!({"lat": l.lat, "lng": l.lng})),
            });

            if let Node::Tension(t) = &node {
                if let Some(obj) = signal_json.as_object_mut() {
                    obj.insert("severity".into(), serde_json::json!(format!("{:?}", t.severity)));
                    obj.insert("category".into(), serde_json::json!(t.category));
                    obj.insert("what_would_help".into(), serde_json::json!(t.what_would_help));
                }
            }

            Json(serde_json::json!({
                "signal": signal_json,
                "evidence": ev_views,
            }))
            .into_response()
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            warn!(error = %e, "Failed to load signal detail");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

// --- Story filter endpoints ---

async fn api_stories_by_category(
    State(state): State<Arc<AppState>>,
    Path(category): Path<String>,
    Query(params): Query<StoriesQuery>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(20).min(100);
    match state.reader.stories_by_category(&category, limit).await {
        Ok(stories) => Json(serde_json::json!({ "stories": stories })).into_response(),
        Err(e) => {
            warn!(error = %e, "Failed to load stories by category");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn api_stories_by_arc(
    State(state): State<Arc<AppState>>,
    Path(arc): Path<String>,
    Query(params): Query<StoriesQuery>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(20).min(100);
    match state.reader.stories_by_arc(&arc, limit).await {
        Ok(stories) => Json(serde_json::json!({ "stories": stories })).into_response(),
        Err(e) => {
            warn!(error = %e, "Failed to load stories by arc");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn api_story_actors(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let uuid = match Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };

    match state.reader.actors_for_story(uuid).await {
        Ok(actors) => Json(serde_json::json!({ "actors": actors })).into_response(),
        Err(e) => {
            warn!(error = %e, "Failed to load story actors");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

// --- Actors API ---

#[derive(Deserialize)]
struct ActorsQuery {
    city: Option<String>,
    limit: Option<u32>,
}

async fn api_actors(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ActorsQuery>,
) -> impl IntoResponse {
    let city = params.city.as_deref().unwrap_or(&state.city);
    let limit = params.limit.unwrap_or(50).min(200);
    match state.reader.actors_active_in_area(city, limit).await {
        Ok(actors) => Json(serde_json::json!({ "actors": actors })).into_response(),
        Err(e) => {
            warn!(error = %e, "Failed to load actors");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn api_actor_detail(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let uuid = match Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };

    match state.reader.actor_detail(uuid).await {
        Ok(Some(actor)) => Json(serde_json::json!({ "actor": actor })).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            warn!(error = %e, "Failed to load actor detail");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn api_actor_stories(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let uuid = match Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };

    match state.reader.actor_stories(uuid, 20).await {
        Ok(stories) => Json(serde_json::json!({ "stories": stories })).into_response(),
        Err(e) => {
            warn!(error = %e, "Failed to load actor stories");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

// --- Tension responses ---

async fn api_tension_responses(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let uuid = match Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };

    match state.reader.tension_responses(uuid).await {
        Ok(responses) => {
            let geojson = nodes_to_geojson(&responses);
            Json(geojson).into_response()
        }
        Err(e) => {
            warn!(error = %e, "Failed to load tension responses");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

// --- Submission API ---

#[derive(Deserialize)]
struct SubmitRequest {
    url: String,
    reason: Option<String>,
    city: Option<String>,
}

const RATE_LIMIT_PER_HOUR: usize = 10;

/// Check rate limit for an IP. Returns true if the request is allowed, false if rate-limited.
/// Prunes expired entries and records the new request if allowed.
fn check_rate_limit(entries: &mut Vec<Instant>, now: Instant, max_per_hour: usize) -> bool {
    let cutoff = now - std::time::Duration::from_secs(3600);
    entries.retain(|t| *t > cutoff);
    if entries.len() >= max_per_hour {
        return false;
    }
    entries.push(now);
    true
}

async fn api_submit(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    Json(body): Json<SubmitRequest>,
) -> impl IntoResponse {
    // Validate URL
    let url = body.url.trim().to_string();
    if url.is_empty() || (!url.starts_with("http://") && !url.starts_with("https://")) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid URL — must start with http:// or https://"})),
        )
            .into_response();
    }

    // Rate limit: 10 submissions per hour per IP
    let ip = addr.ip();
    {
        let mut limiter = state.rate_limiter.lock().await;
        let entries = limiter.entry(ip).or_default();
        if !check_rate_limit(entries, Instant::now(), RATE_LIMIT_PER_HOUR) {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({"error": "Rate limit exceeded — max 10 submissions per hour"})),
            )
                .into_response();
        }
    }

    let city = body.city.as_deref().unwrap_or(&state.city).to_string();
    let source_type = infer_source_type(&url);
    let canonical_value = canonical_value_from_url(source_type, &url);
    let canonical_key = format!("{}:{}:{}", city, source_type, canonical_value);

    let now = chrono::Utc::now();
    let source_id = Uuid::new_v4();
    let source = SourceNode {
        id: source_id,
        canonical_key: canonical_key.clone(),
        canonical_value,
        url: Some(url.clone()),
        source_type,
        discovery_method: DiscoveryMethod::HumanSubmission,
        city: city.clone(),
        created_at: now,
        last_scraped: None,
        last_produced_signal: None,
        signals_produced: 0,
        signals_corroborated: 0,
        consecutive_empty_runs: 0,
        active: true,
        gap_context: body.reason.clone().map(|r| format!("Submission: {r}")),
        weight: 0.5,
        cadence_hours: None,
        avg_signals_per_scrape: 0.0,
        total_cost_cents: 0,
        last_cost_cents: 0,
        taxonomy_stats: None,
    };

    if let Err(e) = state.writer.upsert_source(&source).await {
        warn!(error = %e, "Failed to create submitted source");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    // If reason is non-empty, create a Submission node for investigation
    let reason = body.reason.filter(|r| !r.trim().is_empty());
    if reason.is_some() {
        let submission = SubmissionNode {
            id: Uuid::new_v4(),
            url: url.clone(),
            reason: reason.clone(),
            city: city.clone(),
            submitted_at: now,
        };
        if let Err(e) = state.writer.upsert_submission(&submission, &canonical_key).await {
            warn!(error = %e, "Failed to create submission node");
            // Source was created; submission linkage is non-critical
        }
    }

    info!(url, city, reason = reason.as_deref().unwrap_or(""), "Human submission received");

    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({
            "status": "accepted",
            "source_id": source_id.to_string(),
        })),
    )
        .into_response()
}

/// Infer SourceType from a URL (mirrors discovery.rs logic).
fn infer_source_type(url: &str) -> SourceType {
    if url.contains("instagram.com") {
        SourceType::Instagram
    } else if url.contains("facebook.com") {
        SourceType::Facebook
    } else if url.contains("reddit.com") {
        SourceType::Reddit
    } else if url.contains("tiktok.com") {
        SourceType::TikTok
    } else if url.contains("twitter.com") || url.contains("x.com") {
        SourceType::Twitter
    } else if url.contains("bsky.app") {
        SourceType::Bluesky
    } else {
        SourceType::Web
    }
}

/// Extract the canonical value from a URL for deduplication.
fn canonical_value_from_url(source_type: SourceType, url: &str) -> String {
    match source_type {
        SourceType::Instagram => {
            // https://www.instagram.com/{username}/ → username
            url.split("instagram.com/")
                .nth(1)
                .unwrap_or(url)
                .trim_matches('/')
                .split('/')
                .next()
                .unwrap_or(url)
                .to_lowercase()
        }
        SourceType::Reddit => {
            // https://reddit.com/r/{subreddit} → subreddit
            if let Some(rest) = url.split("/r/").nth(1) {
                rest.trim_matches('/')
                    .split('/')
                    .next()
                    .unwrap_or(url)
                    .to_lowercase()
            } else {
                url.to_lowercase()
            }
        }
        _ => url.to_string(),
    }
}

// --- Editions API ---

#[derive(Deserialize)]
struct EditionsQuery {
    city: Option<String>,
    limit: Option<u32>,
}

async fn api_editions(
    State(state): State<Arc<AppState>>,
    Query(params): Query<EditionsQuery>,
) -> impl IntoResponse {
    let city = params.city.as_deref().unwrap_or(&state.city);
    let limit = params.limit.unwrap_or(10).min(50);
    match state.reader.list_editions(city, limit).await {
        Ok(editions) => Json(serde_json::json!({ "editions": editions })).into_response(),
        Err(e) => {
            warn!(error = %e, "Failed to load editions");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn api_edition_latest(
    State(state): State<Arc<AppState>>,
    Query(params): Query<EditionsQuery>,
) -> impl IntoResponse {
    let city = params.city.as_deref().unwrap_or(&state.city);
    match state.reader.latest_edition(city).await {
        Ok(Some(edition)) => Json(serde_json::json!({ "edition": edition })).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            warn!(error = %e, "Failed to load latest edition");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn api_edition_detail(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let uuid = match Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };

    match state.reader.edition_detail(uuid).await {
        Ok(Some((edition, stories))) => Json(serde_json::json!({
            "edition": edition,
            "stories": stories,
        }))
        .into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            warn!(error = %e, "Failed to load edition detail");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn quality_dashboard(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    // Basic auth check
    if let Some(auth) = headers.get(header::AUTHORIZATION) {
        if let Ok(auth_str) = auth.to_str() {
            if auth_str.starts_with("Basic ") {
                let decoded = base64_decode(&auth_str[6..]);
                if let Some(creds) = decoded {
                    let expected = format!("{}:{}", state.admin_username, state.admin_password);
                    if creds == expected {
                        return match render_quality_page(&state.reader).await {
                            Ok(html) => (StatusCode::OK, [("content-type", "text/html")], html)
                                .into_response(),
                            Err(e) => {
                                warn!(error = %e, "Failed to render quality dashboard");
                                StatusCode::INTERNAL_SERVER_ERROR.into_response()
                            }
                        };
                    }
                }
            }
        }
    }

    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header(header::WWW_AUTHENTICATE, "Basic realm=\"admin\"")
        .body(axum::body::Body::from("Unauthorized"))
        .unwrap()
        .into_response()
}

async fn render_quality_page(reader: &PublicGraphReader) -> Result<String> {
    let total_count = reader.total_count().await.unwrap_or(0);
    let by_type = reader.count_by_type().await.unwrap_or_default();
    let freshness = reader.freshness_distribution().await.unwrap_or_default();
    let confidence = reader.confidence_distribution().await.unwrap_or_default();
    let type_count = by_type.iter().filter(|(_, c)| *c > 0).count();

    let by_type_strs: Vec<(String, u64)> = by_type
        .iter()
        .map(|(t, c)| (format!("{t}"), *c))
        .collect();

    Ok(render_quality(
        total_count,
        type_count,
        &by_type_strs,
        &freshness,
        &confidence,
    ))
}

// --- View Models ---

pub struct NodeView {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub type_label: String,
    pub type_class: String,
    pub confidence: f32,
    pub corroboration_count: u32,
    pub source_diversity: u32,
    pub external_ratio: f32,
    pub cause_heat: f64,
    pub last_confirmed: String,
    pub action_url: String,

    pub completeness_label: String,
    pub tension_category: Option<String>,
    pub tension_what_would_help: Option<String>,
}

pub struct EvidenceView {
    pub source_url: String,
    pub snippet: Option<String>,
    pub relevance: Option<String>,
    pub evidence_confidence: Option<f32>,
}

fn node_to_view(node: &Node) -> NodeView {
    let meta = node.meta();
    let (type_label, type_class) = match node.node_type() {
        NodeType::Event => ("Event", "event"),
        NodeType::Give => ("Give", "give"),
        NodeType::Ask => ("Ask", "ask"),
        NodeType::Notice => ("Notice", "notice"),
        NodeType::Tension => ("Tension", "tension"),
        NodeType::Evidence => ("Evidence", "evidence"),
    };

    let action_url = match node {
        Node::Event(e) => e.action_url.clone(),
        Node::Give(g) => g.action_url.clone(),
        Node::Ask(a) => a.action_url.clone().unwrap_or_default(),
        Node::Notice(_) => String::new(),
        _ => String::new(),
    };

    let confidence = meta.map(|m| m.confidence).unwrap_or(0.0);

    let has_loc = meta.map(|m| m.location.is_some()).unwrap_or(false);
    let completeness_label = if has_loc && !action_url.is_empty() {
        "Has location, timing, and action link"
    } else if has_loc {
        "Has location (missing action link)"
    } else if !action_url.is_empty() {
        "Has action link (missing location)"
    } else {
        "Limited details available"
    };

    let last_confirmed = meta
        .map(|m| {
            let days = (chrono::Utc::now() - m.last_confirmed_active).num_days();
            if days == 0 {
                "today".to_string()
            } else if days == 1 {
                "yesterday".to_string()
            } else {
                format!("{days} days ago")
            }
        })
        .unwrap_or_else(|| "unknown".to_string());

    let (tension_category, tension_what_would_help) = match node {
        Node::Tension(t) => (t.category.clone(), t.what_would_help.clone()),
        _ => (None, None),
    };

    NodeView {
        id: node.id().to_string(),
        title: node.title().to_string(),
        summary: meta.map(|m| m.summary.clone()).unwrap_or_default(),
        type_label: type_label.to_string(),
        type_class: type_class.to_string(),
        confidence,
        corroboration_count: meta.map(|m| m.corroboration_count).unwrap_or(0),
        source_diversity: meta.map(|m| m.source_diversity).unwrap_or(1),
        external_ratio: meta.map(|m| m.external_ratio).unwrap_or(0.0),
        cause_heat: meta.map(|m| m.cause_heat).unwrap_or(0.0),
        last_confirmed,
        action_url,

        completeness_label: completeness_label.to_string(),
        tension_category,
        tension_what_would_help,
    }
}

fn evidence_to_view(ev: &EvidenceNode) -> EvidenceView {
    EvidenceView {
        source_url: ev.source_url.clone(),
        snippet: ev.snippet.clone(),
        relevance: ev.relevance.clone(),
        evidence_confidence: ev.evidence_confidence,
    }
}

fn nodes_to_geojson(nodes: &[Node]) -> serde_json::Value {
    let features: Vec<serde_json::Value> = nodes
        .iter()
        .filter_map(|node| {
            let meta = node.meta()?;
            let loc = meta.location?;
            Some(serde_json::json!({
                "type": "Feature",
                "geometry": {
                    "type": "Point",
                    "coordinates": [loc.lng, loc.lat]
                },
                "properties": {
                    "id": meta.id.to_string(),
                    "title": meta.title,
                    "summary": meta.summary,
                    "node_type": format!("{}", node.node_type()),
                    "confidence": meta.confidence,
                    "corroboration_count": meta.corroboration_count,
                    "source_diversity": meta.source_diversity,
                    "cause_heat": meta.cause_heat,
                }
            }))
        })
        .collect();

    serde_json::json!({
        "type": "FeatureCollection",
        "features": features,
    })
}

fn base64_decode(input: &str) -> Option<String> {
    // Simple base64 decode for basic auth
    let bytes = base64_decode_bytes(input)?;
    String::from_utf8(bytes).ok()
}

fn base64_decode_bytes(input: &str) -> Option<Vec<u8>> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let input = input.trim_end_matches('=');
    let mut output = Vec::new();
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;

    for &b in input.as_bytes() {
        let val = TABLE.iter().position(|&c| c == b)? as u32;
        buf = (buf << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }

    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- infer_source_type tests ---

    #[test]
    fn infer_instagram() {
        assert_eq!(infer_source_type("https://www.instagram.com/mpls_mutual_aid"), SourceType::Instagram);
    }

    #[test]
    fn infer_facebook() {
        assert_eq!(infer_source_type("https://facebook.com/somepage"), SourceType::Facebook);
    }

    #[test]
    fn infer_reddit() {
        assert_eq!(infer_source_type("https://reddit.com/r/Minneapolis"), SourceType::Reddit);
    }

    #[test]
    fn infer_tiktok() {
        assert_eq!(infer_source_type("https://www.tiktok.com/@someuser"), SourceType::TikTok);
    }

    #[test]
    fn infer_twitter() {
        assert_eq!(infer_source_type("https://twitter.com/user"), SourceType::Twitter);
    }

    #[test]
    fn infer_x_dot_com() {
        assert_eq!(infer_source_type("https://x.com/user"), SourceType::Twitter);
    }

    #[test]
    fn infer_bluesky() {
        assert_eq!(infer_source_type("https://bsky.app/profile/someone"), SourceType::Bluesky);
    }

    #[test]
    fn infer_plain_web() {
        assert_eq!(infer_source_type("https://www.startribune.com/article"), SourceType::Web);
    }

    // --- canonical_value_from_url tests ---

    #[test]
    fn canonical_instagram_username() {
        let val = canonical_value_from_url(SourceType::Instagram, "https://www.instagram.com/MplsMutualAid/");
        assert_eq!(val, "mplsmutualaid");
    }

    #[test]
    fn canonical_instagram_with_path() {
        let val = canonical_value_from_url(SourceType::Instagram, "https://instagram.com/user123/reels");
        assert_eq!(val, "user123");
    }

    #[test]
    fn canonical_reddit_subreddit() {
        let val = canonical_value_from_url(SourceType::Reddit, "https://reddit.com/r/Minneapolis/");
        assert_eq!(val, "minneapolis");
    }

    #[test]
    fn canonical_reddit_with_post_path() {
        let val = canonical_value_from_url(SourceType::Reddit, "https://www.reddit.com/r/TwinCities/comments/abc123");
        assert_eq!(val, "twincities");
    }

    #[test]
    fn canonical_web_returns_full_url() {
        let url = "https://www.startribune.com/some-article";
        let val = canonical_value_from_url(SourceType::Web, url);
        assert_eq!(val, url);
    }

    // --- rate limiter tests ---

    #[test]
    fn rate_limit_allows_under_limit() {
        let mut entries = Vec::new();
        let now = Instant::now();
        for _ in 0..9 {
            assert!(check_rate_limit(&mut entries, now, 10));
        }
        assert_eq!(entries.len(), 9);
    }

    #[test]
    fn rate_limit_allows_exactly_at_limit() {
        let mut entries = Vec::new();
        let now = Instant::now();
        for _ in 0..10 {
            assert!(check_rate_limit(&mut entries, now, 10));
        }
        assert_eq!(entries.len(), 10);
    }

    #[test]
    fn rate_limit_rejects_over_limit() {
        let mut entries = Vec::new();
        let now = Instant::now();
        for _ in 0..10 {
            assert!(check_rate_limit(&mut entries, now, 10));
        }
        // 11th should be rejected
        assert!(!check_rate_limit(&mut entries, now, 10));
        // entries should not grow past 10
        assert_eq!(entries.len(), 10);
    }

    #[test]
    fn rate_limit_expires_old_entries() {
        let mut entries = Vec::new();
        let old = Instant::now() - std::time::Duration::from_secs(3601);
        // Simulate 10 old entries
        for _ in 0..10 {
            entries.push(old);
        }
        // New request should be allowed because old ones expired
        let now = Instant::now();
        assert!(check_rate_limit(&mut entries, now, 10));
        // Old entries should have been pruned
        assert_eq!(entries.len(), 1);
    }
}
