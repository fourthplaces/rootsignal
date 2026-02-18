use std::sync::Arc;

use anyhow::Result;
use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
};
use tracing::warn;
use uuid::Uuid;

use rootsignal_common::NodeType;
use rootsignal_graph::PublicGraphReader;

use crate::components::{
    evidence_to_view, node_to_view, render_map, render_quality, render_signal_detail,
    render_signals_list, tension_response_to_view, EvidenceView, NodeView, ResponseView,
};
use crate::AppState;

pub async fn map_page() -> impl IntoResponse {
    Html(render_map())
}

pub async fn nodes_page(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.reader.list_recent(100, None).await {
        Ok(nodes) => {
            let view_nodes: Vec<NodeView> = nodes.iter().map(node_to_view).collect();
            Html(render_signals_list(view_nodes))
        }
        Err(e) => {
            warn!(error = %e, "Failed to load nodes");
            Html("<h1>Error loading signals</h1>".to_string())
        }
    }
}

pub async fn node_detail_page(
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
            let response_views: Vec<ResponseView> = if node.node_type() == NodeType::Tension {
                state
                    .reader
                    .tension_responses(uuid)
                    .await
                    .unwrap_or_default()
                    .iter()
                    .map(tension_response_to_view)
                    .collect()
            } else {
                Vec::new()
            };
            (StatusCode::OK, Html(render_signal_detail(view, ev_views, response_views)))
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

pub async fn quality_dashboard(
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
        by_type_strs,
        freshness,
        confidence,
    ))
}

fn base64_decode(input: &str) -> Option<String> {
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
    use crate::components::{EvidenceView, NodeView, ResponseView};
    use crate::components::{render_signal_detail};

    fn make_node_view(type_label: &str, type_class: &str, title: &str) -> NodeView {
        NodeView {
            id: "00000000-0000-0000-0000-000000000001".to_string(),
            title: title.to_string(),
            summary: "Test summary".to_string(),
            type_label: type_label.to_string(),
            type_class: type_class.to_string(),
            confidence: 0.8,
            corroboration_count: 2,
            source_diversity: 2,
            external_ratio: 0.5,
            cause_heat: 0.0,
            last_confirmed: "today".to_string(),
            action_url: String::new(),
            completeness_label: "Has location".to_string(),
            tension_category: None,
            tension_what_would_help: None,
        }
    }

    fn make_response_view(type_label: &str, type_class: &str, title: &str) -> ResponseView {
        ResponseView {
            id: "00000000-0000-0000-0000-000000000002".to_string(),
            title: title.to_string(),
            type_label: type_label.to_string(),
            type_class: type_class.to_string(),
            match_strength: 0.75,
            explanation: "Addresses the need directly".to_string(),
        }
    }

    #[test]
    fn detail_shows_evidence_confidence_percent() {
        let node = make_node_view("Give", "give", "Free food pantry");
        let evidence = vec![
            EvidenceView {
                source_url: "https://instagram.com/p/abc123".to_string(),
                snippet: Some("Serving meals every Tuesday".to_string()),
                relevance: Some("direct".to_string()),
                evidence_confidence: Some(0.92),
            },
        ];
        let html = render_signal_detail(node, evidence, vec![]);
        assert!(html.contains("(92%)"), "should render confidence as percentage");
    }

    #[test]
    fn detail_hides_confidence_when_zero() {
        let node = make_node_view("Ask", "ask", "Need volunteers");
        let evidence = vec![
            EvidenceView {
                source_url: "https://reddit.com/r/mpls/comments/xyz".to_string(),
                snippet: None,
                relevance: None,
                evidence_confidence: Some(0.0),
            },
        ];
        let html = render_signal_detail(node, evidence, vec![]);
        assert!(!html.contains("(0%)"), "should not render 0% confidence");
    }

    #[test]
    fn detail_hides_confidence_when_none() {
        let node = make_node_view("Event", "event", "Block party");
        let evidence = vec![
            EvidenceView {
                source_url: "https://example.com/event".to_string(),
                snippet: None,
                relevance: None,
                evidence_confidence: None,
            },
        ];
        let html = render_signal_detail(node, evidence, vec![]);
        assert!(!html.contains("%)"), "should not render any confidence percentage");
    }

    #[test]
    fn detail_shows_responses_for_tension() {
        let mut node = make_node_view("Tension", "tension", "Bus routes cut");
        node.tension_category = Some("Transit".to_string());
        node.tension_what_would_help = Some("Restore evening service".to_string());

        let responses = vec![
            make_response_view("Give", "give", "Volunteer shuttle service"),
        ];

        let html = render_signal_detail(node, vec![], responses);
        assert!(html.contains("Responses"), "should show Responses heading");
        assert!(html.contains("Volunteer shuttle service"), "should show response title");
        assert!(html.contains("badge-give"), "should show response type badge");
        assert!(html.contains("75% match"), "should show match strength");
        assert!(html.contains("Addresses the need directly"), "should show explanation");
    }

    #[test]
    fn detail_hides_responses_section_when_empty() {
        let node = make_node_view("Tension", "tension", "Noise complaint");
        let html = render_signal_detail(node, vec![], vec![]);
        assert!(!html.contains("Responses"), "should not show Responses heading when empty");
    }

    #[test]
    fn detail_hides_responses_section_for_non_tension() {
        let node = make_node_view("Give", "give", "Free meals");
        let html = render_signal_detail(node, vec![], vec![]);
        assert!(!html.contains("Responses"), "should not show Responses for non-Tension nodes");
    }
}
