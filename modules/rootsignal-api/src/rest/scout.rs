use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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
pub fn spawn_scout_run(client: GraphClient, config: Config, city_slug: String, cancel: Arc<AtomicBool>) {
    // Reset cancel flag at the start of each run
    cancel.store(false, Ordering::Relaxed);

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
        rt.block_on(async move {
            if let Err(e) = run_scout(&client, &config, &city_slug, cancel).await {
                error!(error = %e, "Scout run failed");
            }
        });
    });
}

async fn run_scout(
    client: &GraphClient,
    config: &Config,
    city_slug: &str,
    cancel: Arc<AtomicBool>,
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
        cancel,
    )?;

    let stats = scout.run().await?;
    info!("Scout run complete. {stats}");

    compute_cause_heat(client, 0.7).await?;

    // Stamp completion time on the city
    writer.set_city_scout_completed(city_slug).await?;

    Ok(())
}

/// Start the multi-city scout interval loop in a background thread.
/// Iterates over all active cities, runs one per iteration, sleeps between.
pub fn start_scout_interval(client: GraphClient, config: Config, interval_hours: u64) {
    info!(
        interval_hours,
        "Starting multi-city scout interval loop"
    );

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
        rt.block_on(async move {
            let writer = rootsignal_graph::GraphWriter::new(client.clone());
            loop {
                // List all active cities
                let cities = match writer.list_cities().await {
                    Ok(c) => c.into_iter().filter(|c| c.active).collect::<Vec<_>>(),
                    Err(e) => {
                        error!(error = %e, "Scout interval: failed to list cities");
                        Vec::new()
                    }
                };

                let num_cities = cities.len().max(1);

                // Run one city per iteration to spread load
                for city in &cities {
                    match writer.is_scout_running(&city.slug).await {
                        Ok(true) => {
                            info!(city = city.slug.as_str(), "Scout interval: already running, skipping");
                            continue;
                        }
                        Err(e) => {
                            warn!(city = city.slug.as_str(), error = %e, "Scout interval: lock check failed, skipping");
                            continue;
                        }
                        Ok(false) => {}
                    }

                    info!(city = city.slug.as_str(), "Scout interval: starting run");
                    let cancel = Arc::new(AtomicBool::new(false));
                    if let Err(e) = run_scout(&client, &config, &city.slug, cancel).await {
                        error!(city = city.slug.as_str(), error = %e, "Scout interval run failed");
                    }
                    // Only run one city per iteration
                    break;
                }

                // Sleep: interval_hours / num_cities, minimum 30 minutes
                let sleep_secs = ((interval_hours * 3600) / num_cities as u64).max(30 * 60);
                info!(
                    sleep_minutes = sleep_secs / 60,
                    "Scout interval: sleeping until next run"
                );
                tokio::time::sleep(std::time::Duration::from_secs(sleep_secs)).await;
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

    // Check if a run is already in progress for this city
    let city_slug = &state.config.city;
    match state.writer.is_scout_running(city_slug).await {
        Ok(true) => {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({"error": "Scout run already in progress"})),
            )
                .into_response();
        }
        Err(e) => {
            warn!(error = %e, "Failed to check scout lock");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Failed to check scout lock"})),
            )
                .into_response();
        }
        Ok(false) => {}
    }

    spawn_scout_run(
        state.graph_client.clone(),
        state.config.clone(),
        city_slug.clone(),
        state.scout_cancel.clone(),
    );

    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({"status": "started"})),
    )
        .into_response()
}

pub async fn scout_status(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    if !check_admin_auth(&headers, &state.config.admin_username, &state.config.admin_password) {
        return axum::response::Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header(header::WWW_AUTHENTICATE, "Basic realm=\"admin\"")
            .body(axum::body::Body::from("Unauthorized"))
            .unwrap()
            .into_response();
    }

    match state.writer.is_scout_running(&state.config.city).await {
        Ok(running) => {
            Json(serde_json::json!({"running": running})).into_response()
        }
        Err(e) => {
            warn!(error = %e, "Failed to check scout lock");
            Json(serde_json::json!({"running": false, "error": "Failed to check lock"})).into_response()
        }
    }
}

fn check_admin_auth(headers: &axum::http::HeaderMap, username: &str, password: &str) -> bool {
    use base64::Engine;

    let Some(auth) = headers.get(header::AUTHORIZATION) else { return false };
    let Ok(auth_str) = auth.to_str() else { return false };
    if !auth_str.starts_with("Basic ") { return false; }

    let encoded = &auth_str[6..];
    let decoded_bytes = match base64::engine::general_purpose::STANDARD.decode(encoded) {
        Ok(b) => b,
        Err(_) => return false,
    };
    let decoded = match String::from_utf8(decoded_bytes) {
        Ok(s) => s,
        Err(_) => return false,
    };

    let expected = format!("{username}:{password}");
    crate::auth::constant_time_eq(decoded.as_bytes(), expected.as_bytes())
}
