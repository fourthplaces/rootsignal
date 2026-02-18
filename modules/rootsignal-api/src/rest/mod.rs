pub mod scout;
pub mod submit;

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde::Deserialize;
use tracing::warn;
use uuid::Uuid;

use rootsignal_common::{EvidenceNode, Node, NodeType};

use crate::AppState;

// --- Query structs ---

#[derive(Deserialize)]
pub struct NearQuery {
    lat: f64,
    lng: f64,
    radius: Option<f64>,
    types: Option<String>,
}

#[derive(Deserialize)]
pub struct StoriesQuery {
    limit: Option<u32>,
    status: Option<String>,
}

#[derive(Deserialize)]
pub struct SignalsQuery {
    limit: Option<u32>,
    types: Option<String>,
}

#[derive(Deserialize)]
pub struct ActorsQuery {
    city: Option<String>,
    limit: Option<u32>,
}

#[derive(Deserialize)]
pub struct EditionsQuery {
    city: Option<String>,
    limit: Option<u32>,
}

// --- Helpers ---

fn parse_node_types(types: &str) -> Vec<NodeType> {
    types
        .split(',')
        .filter_map(|s| match s.trim() {
            "Event" | "event" => Some(NodeType::Event),
            "Give" | "give" => Some(NodeType::Give),
            "Ask" | "ask" => Some(NodeType::Ask),
            "Notice" | "notice" => Some(NodeType::Notice),
            "Tension" | "tension" => Some(NodeType::Tension),
            _ => None,
        })
        .collect()
}

pub fn nodes_to_geojson(nodes: &[Node]) -> serde_json::Value {
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

// --- Handlers ---

pub async fn api_nodes_near(
    State(state): State<Arc<AppState>>,
    Query(params): Query<NearQuery>,
) -> impl IntoResponse {
    let radius = params.radius.unwrap_or(10.0).min(50.0);
    let node_types: Option<Vec<NodeType>> = params.types.as_ref().map(|t| parse_node_types(t));

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

pub async fn api_stories(
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

pub async fn api_story_detail(
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

pub async fn api_story_signals(
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

pub async fn api_signals(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SignalsQuery>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(50).min(200);
    let node_types: Option<Vec<NodeType>> = params.types.as_ref().map(|t| parse_node_types(t));

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

pub async fn api_signal_detail(
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

pub async fn api_stories_by_category(
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

pub async fn api_stories_by_arc(
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

pub async fn api_story_actors(
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

pub async fn api_actors(
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

pub async fn api_actor_detail(
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

pub async fn api_actor_stories(
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

pub async fn api_tension_responses(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let uuid = match Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };

    match state.reader.tension_responses(uuid).await {
        Ok(responses) => {
            let items: Vec<serde_json::Value> = responses
                .iter()
                .filter_map(|tr| {
                    let meta = tr.node.meta()?;
                    let loc = meta.location;
                    Some(serde_json::json!({
                        "id": meta.id.to_string(),
                        "title": meta.title,
                        "summary": meta.summary,
                        "node_type": format!("{}", tr.node.node_type()),
                        "confidence": meta.confidence,
                        "match_strength": tr.match_strength,
                        "explanation": tr.explanation,
                        "location": loc.map(|l| serde_json::json!({"lat": l.lat, "lng": l.lng})),
                    }))
                })
                .collect();
            Json(serde_json::json!({ "responses": items })).into_response()
        }
        Err(e) => {
            warn!(error = %e, "Failed to load tension responses");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub async fn api_editions(
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

pub async fn api_edition_latest(
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

pub async fn api_edition_detail(
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
