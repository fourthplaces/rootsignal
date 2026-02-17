use std::sync::Arc;

use anyhow::Result;
use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Json, Response},
    routing::get,
    Router,
};
use serde::Deserialize;
use tower_http::set_header::SetResponseHeaderLayer;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

use rootsignal_common::{Config, EvidenceNode, Node, NodeType, StoryNode};
use rootsignal_graph::{GraphClient, PublicGraphReader};

mod templates;
use templates::*;

// --- App State ---

struct AppState {
    reader: PublicGraphReader,
    admin_username: String,
    admin_password: String,
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
        reader: PublicGraphReader::new(client),
        admin_username: config.admin_username,
        admin_password: config.admin_password,
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
        .route("/api/signals", get(api_signals))
        .route("/api/signals/{id}", get(api_signal_detail))
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
    axum::serve(listener, app).await?;

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
    let roles = reader.audience_role_distribution().await.unwrap_or_default();

    let type_count = by_type.iter().filter(|(_, c)| *c > 0).count();
    let role_count = roles.len();

    let by_type_strs: Vec<(String, u64)> = by_type
        .iter()
        .map(|(t, c)| (format!("{t}"), *c))
        .collect();

    Ok(render_quality(
        total_count,
        type_count,
        role_count,
        &by_type_strs,
        &freshness,
        &confidence,
        &roles,
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
    pub last_confirmed: String,
    pub action_url: String,
    pub audience_roles: Vec<String>,
    pub source_trust_label: String,
    pub completeness_label: String,
}

pub struct EvidenceView {
    pub source_url: String,
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
    let source_trust = meta.map(|m| m.source_trust).unwrap_or(0.0);
    let source_trust_label = if source_trust >= 0.85 {
        "Government / official source"
    } else if source_trust >= 0.7 {
        "Established organization"
    } else if source_trust >= 0.5 {
        "Community source"
    } else {
        "Unverified source"
    };

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

    NodeView {
        id: node.id().to_string(),
        title: node.title().to_string(),
        summary: meta.map(|m| m.summary.clone()).unwrap_or_default(),
        type_label: type_label.to_string(),
        type_class: type_class.to_string(),
        confidence,
        corroboration_count: meta.map(|m| m.corroboration_count).unwrap_or(0),
        last_confirmed,
        action_url,
        audience_roles: meta
            .map(|m| m.audience_roles.iter().map(|r| format!("{r}")).collect())
            .unwrap_or_default(),
        source_trust_label: source_trust_label.to_string(),
        completeness_label: completeness_label.to_string(),
    }
}

fn evidence_to_view(ev: &EvidenceNode) -> EvidenceView {
    EvidenceView {
        source_url: ev.source_url.clone(),
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
