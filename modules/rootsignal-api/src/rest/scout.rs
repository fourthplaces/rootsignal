use std::sync::Arc;

use axum::{
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Json},
};
use tracing::{error, info, warn};

use rootsignal_common::Config;
use rootsignal_graph::{cause_heat::compute_cause_heat, GraphClient};
use rootsignal_scout::scout::Scout;

use crate::AppState;

/// Spawn a scout run in a dedicated thread.
/// Returns immediately. The scout lock prevents concurrent runs.
pub fn spawn_scout_run(client: GraphClient, config: Config, city_slug: String) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
        rt.block_on(async move {
            if let Err(e) = run_scout(&client, &config, &city_slug).await {
                error!(error = %e, "Scout run failed");
            }
        });
    });
}

async fn run_scout(
    client: &GraphClient,
    config: &Config,
    city_slug: &str,
) -> anyhow::Result<()> {
    let writer = rootsignal_graph::GraphWriter::new(client.clone());

    let city_node = writer
        .get_city(city_slug)
        .await?
        .ok_or_else(|| anyhow::anyhow!("City '{}' not found in graph", city_slug))?;

    info!(city = city_slug, "Scout run starting");

    let scout = Scout::new(
        client.clone(),
        &config.anthropic_api_key,
        &config.voyage_api_key,
        &config.tavily_api_key,
        &config.apify_api_key,
        city_node,
        config.daily_budget_cents,
    )?;

    let stats = scout.run().await?;
    info!("Scout run complete. {stats}");

    compute_cause_heat(client, 0.7).await?;

    Ok(())
}

/// Start the scout interval loop in a background thread.
/// Runs scout every `interval_hours`, sleeping between runs.
pub fn start_scout_interval(client: GraphClient, config: Config, interval_hours: u64) {
    let city_slug = config.city.clone();
    info!(
        interval_hours,
        city = city_slug.as_str(),
        "Starting scout interval loop"
    );

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
        rt.block_on(async move {
            loop {
                info!(city = city_slug.as_str(), "Scout interval: starting run");

                if let Err(e) = run_scout(&client, &config, &city_slug).await {
                    error!(error = %e, "Scout interval run failed");
                }

                info!(
                    hours = interval_hours,
                    "Scout interval: sleeping until next run"
                );
                tokio::time::sleep(std::time::Duration::from_secs(interval_hours * 3600)).await;
            }
        });
    });
}

// --- HTTP handlers ---

pub async fn scout_run_handler(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    // Basic auth (same as quality dashboard)
    if !check_admin_auth(&headers, &state.config.admin_username, &state.config.admin_password) {
        return axum::response::Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header(header::WWW_AUTHENTICATE, "Basic realm=\"admin\"")
            .body(axum::body::Body::from("Unauthorized"))
            .unwrap()
            .into_response();
    }

    // Check that API keys are configured
    if state.config.anthropic_api_key.is_empty()
        || state.config.voyage_api_key.is_empty()
        || state.config.tavily_api_key.is_empty()
    {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "Scout API keys not configured (need ANTHROPIC_API_KEY, VOYAGE_API_KEY, TAVILY_API_KEY)"})),
        ).into_response();
    }

    // Check if a run is already in progress
    let lock_held = match state.writer.acquire_scout_lock().await {
        Ok(true) => {
            let _ = state.writer.release_scout_lock().await;
            false
        }
        Ok(false) => true,
        Err(e) => {
            warn!(error = %e, "Failed to check scout lock");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Failed to check scout lock"})),
            )
                .into_response();
        }
    };

    if lock_held {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "Scout run already in progress"})),
        )
            .into_response();
    }

    spawn_scout_run(
        state.graph_client.clone(),
        state.config.clone(),
        state.config.city.clone(),
    );

    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({"status": "started"})),
    )
        .into_response()
}

pub async fn scout_status(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    match state.writer.acquire_scout_lock().await {
        Ok(true) => {
            let _ = state.writer.release_scout_lock().await;
            Json(serde_json::json!({"running": false}))
        }
        Ok(false) => {
            Json(serde_json::json!({"running": true}))
        }
        Err(e) => {
            warn!(error = %e, "Failed to check scout lock");
            Json(serde_json::json!({"running": false, "error": "Failed to check lock"}))
        }
    }
}

fn check_admin_auth(headers: &axum::http::HeaderMap, username: &str, password: &str) -> bool {
    let Some(auth) = headers.get(header::AUTHORIZATION) else { return false };
    let Ok(auth_str) = auth.to_str() else { return false };
    if !auth_str.starts_with("Basic ") { return false; }

    let encoded = &auth_str[6..];
    let decoded = match base64_decode(encoded) {
        Some(d) => d,
        None => return false,
    };

    let expected = format!("{username}:{password}");
    decoded == expected
}

fn base64_decode(input: &str) -> Option<String> {
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

    String::from_utf8(output).ok()
}
