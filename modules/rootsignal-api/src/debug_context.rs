use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use futures::future::join_all;
use serde::Deserialize;
use uuid::Uuid;

use crate::db::scout_run::{self, event_layer, event_summary, json_str};
use crate::investigate::{
    build_event_context, build_run_context, BREAKDOWN_VARIANTS, SAMPLE_VARIANTS,
};
use crate::jwt;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct DebugContextParams {
    seq: Option<i64>,
    run_id: Option<String>,
    node_id: Option<String>,
}

pub async fn debug_context_handler(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Query(params): Query<DebugContextParams>,
) -> impl IntoResponse {
    // Auth: verify JWT cookie (admin-only, same as investigate)
    let cookie_header = headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let claims = jwt::parse_auth_cookie(cookie_header)
        .and_then(|token| state.jwt_service.verify_token(token).ok());

    if claims.is_none() {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let pool = match &state.pg_pool {
        Some(p) => p,
        None => {
            return (StatusCode::SERVICE_UNAVAILABLE, "Database not available").into_response();
        }
    };

    let markdown = match (&params.seq, &params.run_id, &params.node_id) {
        (Some(seq), _, _) => build_seq_context(pool, &state, *seq).await,
        (_, Some(run_id), _) => build_run_debug_context(pool, run_id).await,
        (_, _, Some(node_id)) => build_node_context(pool, &state, node_id).await,
        _ => Err("Provide one of: ?seq=, ?run_id=, or ?node_id=".to_string()),
    };

    match markdown {
        Ok(md) => (
            [(axum::http::header::CONTENT_TYPE, "text/markdown; charset=utf-8")],
            md,
        )
            .into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e).into_response(),
    }
}

// ---------------------------------------------------------------------------
// ?seq= — event + causal tree + referenced signals
// ---------------------------------------------------------------------------

async fn build_seq_context(
    pool: &sqlx::PgPool,
    state: &AppState,
    seq: i64,
) -> Result<String, String> {
    let (tree_rows, _root_seq) = scout_run::causal_tree(pool, seq)
        .await
        .map_err(|e| format!("Failed to load causal tree: {e}"))?;

    let selected_event = tree_rows.iter().find(|r| r.seq == seq);
    let standalone;
    let event_ref = match selected_event {
        Some(e) => e,
        None => {
            standalone = scout_run::get_event_by_seq(pool, seq)
                .await
                .map_err(|e| format!("Failed to load event: {e}"))?;
            match &standalone {
                Some(e) => e,
                None => return Err("Event not found".to_string()),
            }
        }
    };

    let mut md = format!("# Debug Context: Event seq={seq}\n\n");
    md.push_str(&build_event_context(event_ref, &tree_rows));

    // Auto-resolve signal IDs referenced in the event payload
    let signal_ids = extract_signal_ids(&event_ref.data);
    if !signal_ids.is_empty() {
        md.push_str("\n## Referenced Signals\n\n");
        for id in &signal_ids {
            match state.reader.get_signal_by_id(*id).await {
                Ok(Some(node)) => {
                    md.push_str(&format_signal_markdown(*id, &node));
                }
                Ok(None) => {
                    md.push_str(&format!("### Signal `{id}`\n\nNot found in graph.\n\n"));
                }
                Err(e) => {
                    md.push_str(&format!("### Signal `{id}`\n\nLookup failed: {e}\n\n"));
                }
            }
        }
    }

    // Include findings for referenced signals
    for id in &signal_ids {
        let id_str = id.to_string();
        if let Ok(findings) = state
            .reader
            .list_validation_issues_for_target(&id_str, 10)
            .await
        {
            if !findings.is_empty() {
                md.push_str(&format!("### Findings for `{id}`\n\n"));
                for f in &findings {
                    md.push_str(&format!(
                        "- **{}** ({}): {} — {}\n",
                        f.issue_type, f.severity, f.description, f.suggested_action
                    ));
                }
                md.push('\n');
            }
        }
    }

    Ok(md)
}

// ---------------------------------------------------------------------------
// ?run_id= — run metadata + stats + event breakdown + samples
// ---------------------------------------------------------------------------

async fn build_run_debug_context(
    pool: &sqlx::PgPool,
    run_id: &str,
) -> Result<String, String> {
    let run = scout_run::find_by_id(pool, run_id)
        .await
        .map_err(|e| format!("Failed to load run: {e}"))?
        .ok_or_else(|| "Run not found".to_string())?;

    // Count key event types (all queries in parallel)
    let count_futures = BREAKDOWN_VARIANTS.iter().map(|variant| {
        let run_id = run_id.to_string();
        async move {
            let count = scout_run::count_events_by_variant(pool, &run_id, variant)
                .await
                .unwrap_or(0);
            (*variant, count)
        }
    });
    let variant_counts: Vec<(&str, i64)> = join_all(count_futures)
        .await
        .into_iter()
        .filter(|(_, count)| *count > 0)
        .collect();

    // Sample notable events (all queries in parallel)
    let sample_futures = SAMPLE_VARIANTS.iter().map(|(label, variant, limit)| {
        let run_id = run_id.to_string();
        async move {
            let rows = scout_run::list_events_by_variant(pool, &run_id, variant, *limit)
                .await
                .unwrap_or_default();
            (*label, rows)
        }
    });
    let sample_events: Vec<(&str, Vec<scout_run::EventRow>)> = join_all(sample_futures)
        .await
        .into_iter()
        .filter(|(_, rows)| !rows.is_empty())
        .collect();

    let mut md = format!("# Debug Context: Run {run_id}\n\n");
    md.push_str(&build_run_context(&run, &variant_counts, &sample_events));

    Ok(md)
}

// ---------------------------------------------------------------------------
// ?node_id= — signal data + events that touched it + findings
// ---------------------------------------------------------------------------

async fn build_node_context(
    pool: &sqlx::PgPool,
    state: &AppState,
    node_id: &str,
) -> Result<String, String> {
    let uuid = Uuid::parse_str(node_id).map_err(|e| format!("Invalid UUID: {e}"))?;

    let mut md = format!("# Debug Context: Node {node_id}\n\n");

    // Signal data
    match state.reader.get_signal_by_id(uuid).await {
        Ok(Some(node)) => {
            md.push_str(&format_signal_markdown(uuid, &node));
        }
        Ok(None) => {
            md.push_str("## Signal\n\nNot found in graph.\n\n");
        }
        Err(e) => {
            md.push_str(&format!("## Signal\n\nLookup failed: {e}\n\n"));
        }
    }

    // Events that touched this node
    match scout_run::list_events_by_node_id(pool, node_id, 50).await {
        Ok(rows) if !rows.is_empty() => {
            md.push_str("## Events Touching This Node\n\n");
            md.push_str("| seq | layer | name | timestamp | summary |\n");
            md.push_str("|-----|-------|------|-----------|----------|\n");
            for e in &rows {
                let n = json_str(&e.data, "type").unwrap_or_else(|| e.event_type.clone());
                let s = event_summary(&n, &e.data);
                let l = event_layer(&e.event_type);
                md.push_str(&format!(
                    "| {} | {} | {} | {} | {} |\n",
                    e.seq,
                    l,
                    n,
                    e.ts,
                    s.as_deref().unwrap_or("-"),
                ));
            }
            md.push('\n');
        }
        Ok(_) => {
            md.push_str("## Events Touching This Node\n\nNo events found.\n\n");
        }
        Err(e) => {
            md.push_str(&format!(
                "## Events Touching This Node\n\nQuery failed: {e}\n\n"
            ));
        }
    }

    // Findings
    match state
        .reader
        .list_validation_issues_for_target(node_id, 20)
        .await
    {
        Ok(findings) if !findings.is_empty() => {
            md.push_str("## Supervisor Findings\n\n");
            for f in &findings {
                md.push_str(&format!(
                    "- **{}** ({}, {}): {} — {}\n",
                    f.issue_type, f.severity, f.status, f.description, f.suggested_action
                ));
            }
            md.push('\n');
        }
        Ok(_) => {}
        Err(e) => {
            md.push_str(&format!("## Supervisor Findings\n\nQuery failed: {e}\n\n"));
        }
    }

    Ok(md)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract UUIDs from known signal-referencing fields in an event payload.
fn extract_signal_ids(data: &serde_json::Value) -> Vec<Uuid> {
    let fields = ["node_id", "signal_id", "matched_id", "existing_id"];
    let mut ids = Vec::new();
    for field in &fields {
        if let Some(val) = data.get(field).and_then(|v| v.as_str()) {
            if let Ok(uuid) = Uuid::parse_str(val) {
                if !ids.contains(&uuid) {
                    ids.push(uuid);
                }
            }
        }
    }
    ids
}

/// Format a signal Node as readable markdown.
fn format_signal_markdown(id: Uuid, node: &rootsignal_common::Node) -> String {
    let mut md = format!("### Signal `{id}` — {:?}\n\n", node.node_type());
    if let Some(meta) = node.meta() {
        md.push_str(&format!("- **Title**: {}\n", meta.title));
        md.push_str(&format!("- **Summary**: {}\n", meta.summary));
        md.push_str(&format!("- **Confidence**: {:.2}\n", meta.confidence));
        if let Some(ref cat) = meta.category {
            md.push_str(&format!("- **Category**: {cat}\n"));
        }
        md.push_str(&format!("- **Source URL**: {}\n", meta.source_url));
        if let Some(ref loc) = meta.about_location_name {
            md.push_str(&format!("- **Location**: {loc}\n"));
        }
        md.push_str(&format!("- **Review Status**: {:?}\n", meta.review_status));
        md.push_str(&format!("- **Extracted At**: {}\n", meta.extracted_at.to_rfc3339()));
        if let Some(pub_at) = meta.published_at {
            md.push_str(&format!("- **Published At**: {}\n", pub_at.to_rfc3339()));
        }
        md.push_str(&format!(
            "- **Last Confirmed Active**: {}\n",
            meta.last_confirmed_active.to_rfc3339()
        ));
    }
    md.push('\n');
    md
}
