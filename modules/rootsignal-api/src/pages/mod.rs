use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use axum::{
    extract::{ConnectInfo, Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
};
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::{CityNode, NodeType};
use rootsignal_graph::PublicGraphReader;

use crate::auth::{self, AdminSession};
use crate::components::{
    evidence_to_view, node_to_view, render_cities, render_city_detail, render_login, render_map,
    render_quality, render_signal_detail, render_signals_list, render_stories_list,
    render_story_detail, render_verify, story_to_view, tension_response_to_view, CityView,
    EvidenceView, NodeView, ResponseView, SchedulePreview, ScheduledSourceView, SourceView,
    StoryView,
};
use crate::rest::submit::check_rate_limit;
use crate::AppState;

/// Test phone number — only available in debug builds.
#[cfg(debug_assertions)]
const TEST_PHONE: Option<&str> = Some("+1234567890");
#[cfg(not(debug_assertions))]
const TEST_PHONE: Option<&str> = None;

/// Max auth attempts per IP per hour.
const AUTH_RATE_LIMIT_PER_HOUR: usize = 10;

// --- Auth pages (no AdminSession required) ---

pub async fn login_page() -> impl IntoResponse {
    Html(render_login(None))
}

pub async fn login_submit(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    axum::Form(form): axum::Form<LoginForm>,
) -> Response {
    // Rate limit auth attempts
    {
        let mut limiter = state.rate_limiter.lock().await;
        let entries = limiter.entry(addr.ip()).or_default();
        if !check_rate_limit(entries, Instant::now(), AUTH_RATE_LIMIT_PER_HOUR) {
            return Html(render_login(Some("Too many attempts. Try again later.".to_string())))
                .into_response();
        }
    }

    let phone = form.phone.trim().to_string();

    // Check the number is in the allowlist
    if !state.config.admin_numbers.contains(&phone) {
        return Html(render_login(Some("Phone number not authorized.".to_string()))).into_response();
    }

    // Test number: skip Twilio, go straight to verify (debug builds only)
    if let Some(test_phone) = TEST_PHONE {
        if phone == test_phone {
            return Html(render_verify(phone, None)).into_response();
        }
    }

    // Send OTP via Twilio
    match &state.twilio {
        Some(twilio) => match twilio.send_otp(&phone).await {
            Ok(_) => Html(render_verify(phone, None)).into_response(),
            Err(e) => {
                warn!(error = e, phone = %phone, "Failed to send OTP");
                Html(render_login(Some(format!("Failed to send code: {e}")))).into_response()
            }
        },
        None => {
            // No Twilio configured — show error
            Html(render_login(Some(
                "SMS not configured. Set TWILIO_* env vars.".to_string(),
            )))
            .into_response()
        }
    }
}

pub async fn verify_submit(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    axum::Form(form): axum::Form<VerifyForm>,
) -> Response {
    // Rate limit verify attempts
    {
        let mut limiter = state.rate_limiter.lock().await;
        let entries = limiter.entry(addr.ip()).or_default();
        if !check_rate_limit(entries, Instant::now(), AUTH_RATE_LIMIT_PER_HOUR) {
            return Html(render_verify(
                form.phone.clone(),
                Some("Too many attempts. Try again later.".to_string()),
            ))
            .into_response();
        }
    }

    let phone = form.phone.trim().to_string();
    let code = form.code.trim().to_string();

    // Check allowlist again
    if !state.config.admin_numbers.contains(&phone) {
        return Redirect::to("/admin/login").into_response();
    }

    // Test number: accept any 6-digit code (debug builds only)
    let verified = if TEST_PHONE.is_some_and(|tp| phone == tp) {
        code.len() == 6 && code.chars().all(|c| c.is_ascii_digit())
    } else {
        match &state.twilio {
            Some(twilio) => twilio.verify_otp(&phone, &code).await.is_ok(),
            None => false,
        }
    };

    if verified {
        let secret = auth::session_secret(&state.config);
        let cookie = auth::session_cookie(&phone, secret);
        Response::builder()
            .status(StatusCode::SEE_OTHER)
            .header("location", "/admin")
            .header("set-cookie", cookie)
            .body(axum::body::Body::empty())
            .unwrap()
    } else {
        Html(render_verify(
            phone,
            Some("Invalid code. Please try again.".to_string()),
        ))
        .into_response()
    }
}

pub async fn logout() -> Response {
    let cookie = auth::clear_session_cookie();
    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header("location", "/admin/login")
        .header("set-cookie", cookie)
        .body(axum::body::Body::empty())
        .unwrap()
}

#[derive(serde::Deserialize)]
pub struct LoginForm {
    pub phone: String,
}

#[derive(serde::Deserialize)]
pub struct VerifyForm {
    pub phone: String,
    pub code: String,
}

// --- Protected admin pages (AdminSession required) ---

pub async fn map_page(_session: AdminSession) -> impl IntoResponse {
    Html(render_map())
}

pub async fn nodes_page(
    _session: AdminSession,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
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
    _session: AdminSession,
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
            (
                StatusCode::OK,
                Html(render_signal_detail(view, ev_views, response_views)),
            )
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

pub async fn stories_page(
    _session: AdminSession,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    match state.reader.top_stories_by_energy(50, None).await {
        Ok(stories) => {
            let story_ids: Vec<uuid::Uuid> = stories.iter().map(|s| s.id).collect();
            let ev_counts = state
                .reader
                .story_evidence_counts(&story_ids)
                .await
                .unwrap_or_default();
            let views: Vec<StoryView> = stories
                .iter()
                .map(|s| {
                    let ec = ev_counts
                        .iter()
                        .find(|(id, _)| *id == s.id)
                        .map(|(_, c)| *c)
                        .unwrap_or(0);
                    story_to_view(s, ec)
                })
                .collect();
            Html(render_stories_list(views))
        }
        Err(e) => {
            warn!(error = %e, "Failed to load stories");
            Html("<h1>Error loading stories</h1>".to_string())
        }
    }
}

pub async fn story_detail_page(
    _session: AdminSession,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let uuid = match Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return (StatusCode::BAD_REQUEST, Html("Invalid ID".to_string())),
    };

    match state.reader.get_story_with_signals(uuid).await {
        Ok(Some((story, signals))) => {
            let story_ids = vec![story.id];
            let ev_counts = state
                .reader
                .story_evidence_counts(&story_ids)
                .await
                .unwrap_or_default();
            let ec = ev_counts
                .first()
                .map(|(_, c)| *c)
                .unwrap_or(0);
            let story_view = story_to_view(&story, ec);
            let signal_views: Vec<NodeView> = signals.iter().map(node_to_view).collect();
            (
                StatusCode::OK,
                Html(render_story_detail(story_view, signal_views)),
            )
        }
        Ok(None) => (StatusCode::NOT_FOUND, Html("Story not found".to_string())),
        Err(e) => {
            warn!(error = %e, "Failed to load story detail");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html("Error loading story".to_string()),
            )
        }
    }
}

pub async fn cities_page(
    _session: AdminSession,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    // Check if scout is currently running
    let scout_running = match state.writer.acquire_scout_lock().await {
        Ok(true) => {
            let _ = state.writer.release_scout_lock().await;
            false
        }
        Ok(false) => true,
        Err(_) => false,
    };

    match state.writer.list_cities().await {
        Ok(cities) => {
            let views: Vec<CityView> = cities
                .iter()
                .map(|c| CityView {
                    name: c.name.clone(),
                    slug: c.slug.clone(),
                    center_lat: c.center_lat,
                    center_lng: c.center_lng,
                    radius_km: c.radius_km,
                    geo_terms: c.geo_terms.join(", "),
                    active: c.active,
                    scout_running,
                })
                .collect();
            Html(render_cities(views))
        }
        Err(e) => {
            warn!(error = %e, "Failed to load cities");
            Html("<h1>Error loading cities</h1>".to_string())
        }
    }
}

#[derive(serde::Deserialize)]
pub struct CityDetailQuery {
    #[serde(default = "default_tab")]
    pub tab: String,
}

fn default_tab() -> String {
    "signals".to_string()
}

pub async fn city_detail_page(
    _session: AdminSession,
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
    Query(params): Query<CityDetailQuery>,
) -> impl IntoResponse {
    let city = match state.writer.get_city(&slug).await {
        Ok(Some(c)) => c,
        Ok(None) => return (StatusCode::NOT_FOUND, Html("City not found".to_string())),
        Err(e) => {
            warn!(error = %e, "Failed to load city");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html("Error loading city".to_string()),
            );
        }
    };

    let scout_running = match state.writer.acquire_scout_lock().await {
        Ok(true) => {
            let _ = state.writer.release_scout_lock().await;
            false
        }
        Ok(false) => true,
        Err(_) => false,
    };

    let city_view = CityView {
        name: city.name.clone(),
        slug: city.slug.clone(),
        center_lat: city.center_lat,
        center_lng: city.center_lng,
        radius_km: city.radius_km,
        geo_terms: city.geo_terms.join(", "),
        active: city.active,
        scout_running,
    };

    let tab = match params.tab.as_str() {
        "stories" => "stories".to_string(),
        "sources" => "sources".to_string(),
        _ => "signals".to_string(),
    };

    let signals = if tab == "signals" {
        state
            .reader
            .list_recent(100, None)
            .await
            .unwrap_or_default()
            .iter()
            .map(node_to_view)
            .collect()
    } else {
        Vec::new()
    };

    let stories = if tab == "stories" {
        let raw = state
            .reader
            .top_stories_by_energy(50, None)
            .await
            .unwrap_or_default();
        let story_ids: Vec<uuid::Uuid> = raw.iter().map(|s| s.id).collect();
        let ev_counts = state
            .reader
            .story_evidence_counts(&story_ids)
            .await
            .unwrap_or_default();
        raw.iter()
            .map(|s| {
                let ec = ev_counts
                    .iter()
                    .find(|(id, _)| *id == s.id)
                    .map(|(_, c)| *c)
                    .unwrap_or(0);
                story_to_view(s, ec)
            })
            .collect()
    } else {
        Vec::new()
    };

    let (sources, schedule) = if tab == "sources" {
        match state.writer.get_active_sources(&slug).await {
            Ok(raw_sources) => {
                let now = chrono::Utc::now();

                // Build source views
                let source_views: Vec<SourceView> = raw_sources
                    .iter()
                    .map(|s| {
                        let effective_weight = s.weight * s.quality_penalty;
                        let cadence = s.cadence_hours.unwrap_or_else(|| {
                            rootsignal_scout::scheduler::cadence_hours_for_weight(effective_weight)
                        });
                        SourceView {
                            canonical_key: s.canonical_key.clone(),
                            canonical_value: s.canonical_value.clone(),
                            url: s.url.clone(),
                            source_type: s.source_type.to_string(),
                            is_query: s.source_type.is_query(),
                            discovery_method: s.discovery_method.to_string(),
                            weight: s.weight,
                            quality_penalty: s.quality_penalty,
                            effective_weight,
                            cadence_hours: cadence,
                            signals_produced: s.signals_produced,
                            signals_corroborated: s.signals_corroborated,
                            consecutive_empty_runs: s.consecutive_empty_runs,
                            last_scraped: s.last_scraped.map(|t| format_relative_time(t, now)),
                            last_produced_signal: s.last_produced_signal.map(|t| format_relative_time(t, now)),
                            gap_context: s.gap_context.clone(),
                        }
                    })
                    .collect();

                // Run scheduler dry-run
                let scheduler = rootsignal_scout::scheduler::SourceScheduler::new();
                let result = scheduler.schedule(&raw_sources, now);

                let scheduled_views: Vec<ScheduledSourceView> = result
                    .scheduled
                    .iter()
                    .filter_map(|ss| {
                        raw_sources.iter().find(|s| s.canonical_key == ss.canonical_key).map(|s| {
                            let effective_weight = s.weight * s.quality_penalty;
                            let cadence = s.cadence_hours.unwrap_or_else(|| {
                                rootsignal_scout::scheduler::cadence_hours_for_weight(effective_weight)
                            });
                            ScheduledSourceView {
                                canonical_value: s.canonical_value.clone(),
                                source_type: s.source_type.to_string(),
                                is_query: s.source_type.is_query(),
                                reason: match ss.reason {
                                    rootsignal_scout::scheduler::ScheduleReason::Cadence => "Cadence".to_string(),
                                    rootsignal_scout::scheduler::ScheduleReason::NeverScraped => "New".to_string(),
                                    rootsignal_scout::scheduler::ScheduleReason::Exploration => "Exploration".to_string(),
                                },
                                weight: effective_weight,
                                cadence_hours: cadence,
                                last_scraped: s.last_scraped.map(|t| format_relative_time(t, now)),
                                hours_until_due: None,
                            }
                        })
                    })
                    .collect();

                let exploration_views: Vec<ScheduledSourceView> = result
                    .exploration
                    .iter()
                    .filter_map(|ss| {
                        raw_sources.iter().find(|s| s.canonical_key == ss.canonical_key).map(|s| {
                            let effective_weight = s.weight * s.quality_penalty;
                            let cadence = s.cadence_hours.unwrap_or_else(|| {
                                rootsignal_scout::scheduler::cadence_hours_for_weight(effective_weight)
                            });
                            ScheduledSourceView {
                                canonical_value: s.canonical_value.clone(),
                                source_type: s.source_type.to_string(),
                                is_query: s.source_type.is_query(),
                                reason: "Exploration".to_string(),
                                weight: effective_weight,
                                cadence_hours: cadence,
                                last_scraped: s.last_scraped.map(|t| format_relative_time(t, now)),
                                hours_until_due: None,
                            }
                        })
                    })
                    .collect();

                let preview = SchedulePreview {
                    total_sources: raw_sources.len(),
                    skipped_count: result.skipped,
                    scheduled: scheduled_views,
                    exploration: exploration_views,
                };

                (source_views, Some(preview))
            }
            Err(e) => {
                warn!(error = %e, "Failed to load sources");
                (Vec::new(), None)
            }
        }
    } else {
        (Vec::new(), None)
    };

    (
        StatusCode::OK,
        Html(render_city_detail(city_view, tab, signals, stories, sources, schedule)),
    )
}

pub async fn create_city(
    _session: AdminSession,
    State(state): State<Arc<AppState>>,
    axum::Form(form): axum::Form<CreateCityForm>,
) -> impl IntoResponse {
    let location = form.location.trim().to_string();
    if location.is_empty() {
        warn!("Empty location submitted");
        return Redirect::to("/admin/cities");
    }

    // Geocode via Nominatim
    let geocode_result = geocode_location(&location).await;
    let (lat, lon, display_name) = match geocode_result {
        Ok(r) => r,
        Err(e) => {
            warn!(location = location.as_str(), error = %e, "Geocoding failed");
            return Redirect::to("/admin/cities");
        }
    };

    // Derive slug: lowercase, non-alphanum → hyphens, dedupe hyphens
    let slug: String = location
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    // Derive geo_terms from comma-split parts of the location
    let geo_terms: Vec<String> = location
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let city = CityNode {
        id: Uuid::new_v4(),
        name: display_name,
        slug,
        center_lat: lat,
        center_lng: lon,
        radius_km: 30.0,
        geo_terms,
        active: true,
        created_at: chrono::Utc::now(),
    };

    if let Err(e) = state.writer.upsert_city(&city).await {
        warn!(error = %e, "Failed to create city");
        return Redirect::to("/admin/cities");
    }

    // Run cold-start bootstrapper (non-fatal if API keys missing)
    let writer = rootsignal_graph::GraphWriter::new(state.graph_client.clone());
    let searcher = rootsignal_scout::scraper::TavilySearcher::new(&state.config.tavily_api_key);
    let bootstrapper = rootsignal_scout::bootstrap::ColdStartBootstrapper::new(
        &writer,
        &searcher,
        &state.config.anthropic_api_key,
        city,
    );
    match bootstrapper.run().await {
        Ok(n) => tracing::info!(sources = n, "Bootstrap complete for new city"),
        Err(e) => warn!(error = %e, "Bootstrap failed (non-fatal, sources can be added later)"),
    }

    Redirect::to("/admin/cities")
}

pub async fn stop_city_scout(
    _session: AdminSession,
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    info!(city = slug.as_str(), "Scout stop requested by admin");
    state.scout_cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    Redirect::to(&format!("/admin/cities/{slug}"))
}

pub async fn reset_scout_lock(
    _session: AdminSession,
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    info!(city = slug.as_str(), "Scout lock reset requested by admin");
    if let Err(e) = state.writer.release_scout_lock().await {
        warn!(error = %e, "Failed to release scout lock");
    }
    Redirect::to(&format!("/admin/cities/{slug}"))
}

pub async fn run_city_scout(
    _session: AdminSession,
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    // Check API keys
    if state.config.anthropic_api_key.is_empty()
        || state.config.voyage_api_key.is_empty()
        || state.config.tavily_api_key.is_empty()
    {
        warn!("Scout API keys not configured");
        return Redirect::to("/admin/cities");
    }

    // Check if already running
    let already_running = match state.writer.acquire_scout_lock().await {
        Ok(true) => {
            let _ = state.writer.release_scout_lock().await;
            false
        }
        Ok(false) => true,
        Err(_) => false,
    };

    if !already_running {
        crate::rest::scout::spawn_scout_run(
            state.graph_client.clone(),
            state.config.clone(),
            slug,
            state.scout_cancel.clone(),
        );
    }

    Redirect::to("/admin/cities")
}

#[derive(serde::Deserialize)]
struct NominatimResult {
    lat: String,
    lon: String,
    display_name: String,
}

async fn geocode_location(location: &str) -> anyhow::Result<(f64, f64, String)> {
    if location.len() > 200 {
        anyhow::bail!("Location input too long (max 200 chars)");
    }
    let client = reqwest::Client::new();
    let resp = client
        .get("https://nominatim.openstreetmap.org/search")
        .query(&[("q", location), ("format", "json"), ("limit", "1")])
        .header("User-Agent", "rootsignal/1.0")
        .send()
        .await?;

    let results: Vec<NominatimResult> = resp.json().await?;
    let first = results
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("No geocoding results for '{}'", location))?;

    let lat: f64 = first.lat.parse()?;
    let lon: f64 = first.lon.parse()?;
    Ok((lat, lon, first.display_name))
}

#[derive(serde::Deserialize)]
pub struct CreateCityForm {
    pub location: String,
}

fn format_relative_time(t: chrono::DateTime<chrono::Utc>, now: chrono::DateTime<chrono::Utc>) -> String {
    let hours = (now - t).num_hours();
    if hours < 1 {
        "just now".to_string()
    } else if hours < 24 {
        format!("{hours}h ago")
    } else {
        let days = hours / 24;
        if days == 1 {
            "yesterday".to_string()
        } else {
            format!("{days}d ago")
        }
    }
}

pub async fn quality_dashboard(
    _session: AdminSession,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    match render_quality_page(&state.reader).await {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            warn!(error = %e, "Failed to render quality dashboard");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
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

#[cfg(test)]
mod tests {
    use crate::components::{EvidenceView, NodeView, ResponseView};
    use crate::components::render_signal_detail;

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
        assert!(html.contains("bg-green-50"), "should show response type badge");
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
