use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Instant;

use async_graphql::{Context, Object, Result, SimpleObject};
use tokio::sync::Mutex;
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::{
    Config, DemandSignal, DiscoveryMethod, ScoutScope, SourceNode, SourceRole,
};
use rootsignal_graph::GraphStore;
use rootsignal_scout::store::{EngineFactory, SignalReaderFactory};
use crate::jwt::{self, JwtService};
use crate::restate_client::RestateClient;
use super::context::AdminGuard;


/// Rate limiter state shared via GraphQL context.
pub struct RateLimiter(pub Mutex<std::collections::HashMap<IpAddr, Vec<Instant>>>);

/// The client IP, extracted from the HTTP request and passed into GraphQL context.
pub struct ClientIp(pub IpAddr);

/// HTTP response headers that mutations can set (e.g., Set-Cookie).
/// Wrapped in a Mutex so mutations can append headers.
pub struct ResponseHeaders(pub Mutex<Vec<(String, String)>>);

pub struct MutationRoot;

// --- Auth result types ---

#[derive(SimpleObject)]
struct SendOtpResult {
    success: bool,
}

#[derive(SimpleObject)]
struct VerifyOtpResult {
    success: bool,
}

#[derive(SimpleObject)]
struct LogoutResult {
    success: bool,
}

#[derive(SimpleObject)]
struct AddSourceResult {
    success: bool,
    source_id: Option<String>,
}

#[derive(SimpleObject)]
struct ScoutResult {
    success: bool,
    message: Option<String>,
}

#[derive(SimpleObject)]
struct SubmitSourceResult {
    success: bool,
    source_id: Option<String>,
}

/// Test phone number — only available in debug builds.
#[cfg(debug_assertions)]
const TEST_PHONE: Option<&str> = Some("+1234567890");
#[cfg(not(debug_assertions))]
const TEST_PHONE: Option<&str> = None;

const AUTH_RATE_LIMIT_PER_HOUR: usize = 10;
const SUBMIT_RATE_LIMIT_PER_HOUR: usize = 10;
const DEMAND_RATE_LIMIT_PER_HOUR: usize = 10;

#[Object]
impl MutationRoot {
    // ========== Auth mutations (no guard) ==========

    /// Send an OTP code to the given phone number.
    async fn send_otp(&self, ctx: &Context<'_>, phone: String) -> Result<SendOtpResult> {
        let phone = phone.trim().to_string();
        let config = ctx.data_unchecked::<Arc<Config>>();

        // Rate limit
        rate_limit_check(ctx, AUTH_RATE_LIMIT_PER_HOUR)?;

        // Check allowlist
        if !config.admin_numbers.contains(&phone) {
            return Ok(SendOtpResult { success: false });
        }

        // Test phone: skip Twilio
        if let Some(test_phone) = TEST_PHONE {
            if phone == test_phone {
                return Ok(SendOtpResult { success: true });
            }
        }

        // Send via Twilio
        let twilio = ctx.data_unchecked::<Option<Arc<twilio::TwilioService>>>();
        match twilio {
            Some(twilio) => match twilio.send_otp(&phone).await {
                Ok(_) => Ok(SendOtpResult { success: true }),
                Err(e) => {
                    warn!(error = e, "Failed to send OTP");
                    Ok(SendOtpResult { success: false })
                }
            },
            None => {
                warn!("Twilio not configured");
                Ok(SendOtpResult { success: false })
            }
        }
    }

    /// Verify an OTP code. On success, sets the JWT cookie via response headers.
    async fn verify_otp(
        &self,
        ctx: &Context<'_>,
        phone: String,
        code: String,
    ) -> Result<VerifyOtpResult> {
        let phone = phone.trim().to_string();
        let code = code.trim().to_string();
        let config = ctx.data_unchecked::<Arc<Config>>();

        // Rate limit
        rate_limit_check(ctx, AUTH_RATE_LIMIT_PER_HOUR)?;

        // Check allowlist
        if !config.admin_numbers.contains(&phone) {
            return Ok(VerifyOtpResult { success: false });
        }

        // Verify OTP
        let verified = if TEST_PHONE.is_some_and(|tp| phone == tp) {
            code.len() == 6 && code.chars().all(|c| c.is_ascii_digit())
        } else {
            let twilio = ctx.data_unchecked::<Option<Arc<twilio::TwilioService>>>();
            match twilio {
                Some(twilio) => twilio.verify_otp(&phone, &code).await.is_ok(),
                None => false,
            }
        };

        if verified {
            let jwt_service = ctx.data_unchecked::<JwtService>();
            let is_admin = config.admin_numbers.contains(&phone);
            let token = jwt_service
                .create_token(&phone, is_admin)
                .map_err(|e| async_graphql::Error::new(format!("Token creation failed: {e}")))?;

            // Set the JWT cookie via response headers
            let headers = ctx.data_unchecked::<Arc<ResponseHeaders>>();
            let mut h = headers.0.lock().await;
            h.push(("set-cookie".to_string(), jwt::jwt_cookie(&token)));

            Ok(VerifyOtpResult { success: true })
        } else {
            Ok(VerifyOtpResult { success: false })
        }
    }

    /// Clear the auth cookie.
    async fn logout(&self, ctx: &Context<'_>) -> Result<LogoutResult> {
        let headers = ctx.data_unchecked::<Arc<ResponseHeaders>>();
        let mut h = headers.0.lock().await;
        h.push(("set-cookie".to_string(), jwt::clear_jwt_cookie()));
        Ok(LogoutResult { success: true })
    }

    // ========== Admin mutations (AdminGuard) ==========

    /// Add a source.
    #[graphql(guard = "AdminGuard")]
    async fn add_source(
        &self,
        ctx: &Context<'_>,
        url: String,
        reason: Option<String>,
    ) -> Result<AddSourceResult> {
        let engine = require_engine(ctx)?;
        let url = url.trim().to_string();

        // Validate URL
        if url.len() > 2048 {
            return Err("URL too long (max 2048 characters)".into());
        }
        let parsed = url::Url::parse(&url).map_err(|_| async_graphql::Error::new("Invalid URL"))?;
        if parsed.scheme() != "http" && parsed.scheme() != "https" {
            return Err("URL must use http or https scheme".into());
        }

        let cv = rootsignal_common::canonical_value(&url);
        let canonical_key = cv.clone();
        let source_id = Uuid::new_v4();
        let now = chrono::Utc::now();

        let source = SourceNode {
            id: source_id,
            canonical_key: canonical_key.clone(),
            canonical_value: cv,
            url: Some(url.clone()),
            discovery_method: DiscoveryMethod::HumanSubmission,
            created_at: now,
            last_scraped: None,
            last_produced_signal: None,
            signals_produced: 0,
            signals_corroborated: 0,
            consecutive_empty_runs: 0,
            active: true,
            gap_context: reason.as_ref().map(|r| format!("Admin: {r}")),
            weight: 0.5,
            cadence_hours: None,
            avg_signals_per_scrape: 0.0,
            quality_penalty: 1.0,
            source_role: SourceRole::default(),
            scrape_count: 0,
        };

        engine
            .emit(rootsignal_scout::domains::discovery::events::DiscoveryEvent::SourceDiscovered {
                source,
                discovered_by: "admin".into(),
            })
            .settled()
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to create source: {e}")))?;

        info!(url, "Source added by admin");

        Ok(AddSourceResult {
            success: true,
            source_id: Some(source_id.to_string()),
        })
    }

    /// Run scout for a task. Loads task by ID, derives scope, dispatches via Restate.
    #[graphql(guard = "AdminGuard")]
    async fn run_scout(&self, ctx: &Context<'_>, task_id: String) -> Result<ScoutResult> {
        let config = ctx.data_unchecked::<Arc<Config>>();

        // Check API keys
        if config.anthropic_api_key.is_empty()
            || config.voyage_api_key.is_empty()
            || config.serper_api_key.is_empty()
        {
            return Ok(ScoutResult {
                success: false,
                message: Some("Scout API keys not configured".to_string()),
            });
        }

        let writer = ctx.data_unchecked::<Arc<GraphStore>>();
        let restate = require_restate(ctx)?;

        // Load the task
        let task = writer
            .get_scout_task(&task_id)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to load task: {e}")))?
            .ok_or_else(|| async_graphql::Error::new(format!("Scout task {task_id} not found")))?;

        // Concurrency guard: reject if another task for the same region is already running
        let running = writer
            .is_region_task_running(&task.context)
            .await
            .map_err(|e| {
                async_graphql::Error::new(format!("Failed to check running status: {e}"))
            })?;
        if running {
            return Ok(ScoutResult {
                success: false,
                message: Some("Another task for this region is already running".to_string()),
            });
        }

        let scope = ScoutScope::from(&task);

        restate
            .run_scout(&task_id, &scope)
            .await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;

        Ok(ScoutResult {
            success: true,
            message: Some(format!("Scout started via Restate for {}", task.context)),
        })
    }

    /// Run an individual scout workflow phase for a task.
    #[graphql(guard = "AdminGuard")]
    async fn run_scout_phase(
        &self,
        ctx: &Context<'_>,
        phase: super::types::ScoutPhase,
        task_id: String,
    ) -> Result<ScoutResult> {
        let config = ctx.data_unchecked::<Arc<Config>>();

        if config.anthropic_api_key.is_empty()
            || config.voyage_api_key.is_empty()
            || config.serper_api_key.is_empty()
        {
            return Ok(ScoutResult {
                success: false,
                message: Some("Scout API keys not configured".to_string()),
            });
        }

        let writer = ctx.data_unchecked::<Arc<GraphStore>>();
        let restate = require_restate(ctx)?;
        let restate_phase: crate::restate_client::ScoutPhase = phase.into();

        // Load the task
        let task = writer
            .get_scout_task(&task_id)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to load task: {e}")))?
            .ok_or_else(|| async_graphql::Error::new(format!("Scout task {task_id} not found")))?;

        // Concurrency guard
        let running = writer
            .is_region_task_running(&task.context)
            .await
            .map_err(|e| {
                async_graphql::Error::new(format!("Failed to check running status: {e}"))
            })?;
        if running {
            return Ok(ScoutResult {
                success: false,
                message: Some("Another task for this region is already running".to_string()),
            });
        }

        let scope = ScoutScope::from(&task);

        restate
            .run_phase(restate_phase, &task_id, &scope)
            .await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;

        Ok(ScoutResult {
            success: true,
            message: Some(format!(
                "{:?} started via Restate for {}",
                phase, task.context
            )),
        })
    }

    /// Stop a running scout workflow via Restate cancellation.
    #[graphql(guard = "AdminGuard")]
    async fn stop_scout(&self, ctx: &Context<'_>, task_id: String) -> Result<ScoutResult> {
        let restate = require_restate(ctx)?;

        match restate
            .cancel_workflow("FullScoutRunWorkflow", &task_id)
            .await
        {
            Ok(()) => Ok(ScoutResult {
                success: true,
                message: Some(format!("Cancel signal sent for task {task_id}")),
            }),
            Err(crate::restate_client::RestateError::Ingress { status, body }) => {
                warn!(status, body = %body, "Restate cancel failed");
                Ok(ScoutResult {
                    success: false,
                    message: Some(format!("Cancel failed (HTTP {status}): {body}")),
                })
            }
            Err(e) => Err(async_graphql::Error::new(e.to_string())),
        }
    }

    /// Reset a stuck scout task status to idle.
    #[graphql(guard = "AdminGuard")]
    async fn reset_scout_status(&self, ctx: &Context<'_>, task_id: String) -> Result<ScoutResult> {
        let writer = ctx.data_unchecked::<Arc<GraphStore>>();
        info!(task_id = task_id.as_str(), "Scout status reset requested");
        writer
            .reset_task_phase_status(&task_id)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to reset status: {e}")))?;
        Ok(ScoutResult {
            success: true,
            message: Some("Status reset to idle".to_string()),
        })
    }

    /// Public source submission (rate-limited, no auth required).
    async fn submit_source(&self, ctx: &Context<'_>, url: String) -> Result<SubmitSourceResult> {
        let engine = require_engine(ctx)?;

        // Rate limit
        rate_limit_check(ctx, SUBMIT_RATE_LIMIT_PER_HOUR)?;

        // Validate URL
        let url = url.trim().to_string();
        if url.len() > 2048 {
            return Err("URL too long (max 2048 characters)".into());
        }
        let parsed = url::Url::parse(&url).map_err(|_| async_graphql::Error::new("Invalid URL"))?;
        if parsed.scheme() != "http" && parsed.scheme() != "https" {
            return Err("URL must use http or https scheme".into());
        }
        // Block private/internal URLs
        if let Some(host) = parsed.host_str() {
            let lower = host.to_lowercase();
            if lower == "localhost" || lower.ends_with(".local") || lower.ends_with(".internal") {
                return Err("URLs pointing to internal hosts are not allowed".into());
            }
        }

        let cv = rootsignal_common::canonical_value(&url);
        let canonical_key = cv.clone();
        let source_id = Uuid::new_v4();
        let now = chrono::Utc::now();

        let source = SourceNode {
            id: source_id,
            canonical_key: canonical_key.clone(),
            canonical_value: cv,
            url: Some(url.clone()),
            discovery_method: DiscoveryMethod::HumanSubmission,
            created_at: now,
            last_scraped: None,
            last_produced_signal: None,
            signals_produced: 0,
            signals_corroborated: 0,
            consecutive_empty_runs: 0,
            active: true,
            gap_context: None,
            weight: 0.5,
            cadence_hours: None,
            avg_signals_per_scrape: 0.0,
            quality_penalty: 1.0,
            source_role: SourceRole::default(),
            scrape_count: 0,
        };

        engine
            .emit(rootsignal_scout::domains::discovery::events::DiscoveryEvent::SourceDiscovered {
                source,
                discovered_by: "human_submission".into(),
            })
            .settled()
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to create source: {e}")))?;

        info!(url, "Human submission received via GraphQL");

        Ok(SubmitSourceResult {
            success: true,
            source_id: Some(source_id.to_string()),
        })
    }

    /// Add a curated tag to a signal.
    ///
    /// TODO: batch_tag_signals was removed from GraphStore; re-implement
    /// once tagging flows through the engine event path.
    #[graphql(guard = "AdminGuard")]
    async fn tag_signal(
        &self,
        _ctx: &Context<'_>,
        _signal_id: Uuid,
        _tag_slug: String,
    ) -> Result<bool> {
        Err(async_graphql::Error::new(
            "tag_signal is temporarily unavailable — pending engine migration",
        ))
    }

    /// Remove a tag from a situation (deletes TAGGED + creates SUPPRESSED_TAG).
    #[graphql(guard = "AdminGuard")]
    async fn untag_situation(
        &self,
        ctx: &Context<'_>,
        situation_id: Uuid,
        tag_slug: String,
    ) -> Result<bool> {
        let writer = ctx.data_unchecked::<Arc<GraphStore>>();
        writer
            .suppress_situation_tag(situation_id, &tag_slug)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to untag situation: {e}")))?;
        Ok(true)
    }

    /// Merge tag B into tag A (repoints all edges, deletes B).
    #[graphql(guard = "AdminGuard")]
    async fn merge_tags(
        &self,
        ctx: &Context<'_>,
        source_slug: String,
        target_slug: String,
    ) -> Result<bool> {
        let writer = ctx.data_unchecked::<Arc<GraphStore>>();
        writer
            .merge_tags(&source_slug, &target_slug)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to merge tags: {e}")))?;
        Ok(true)
    }

    /// Dismiss a supervisor finding (validation issue).
    #[graphql(guard = "AdminGuard")]
    async fn dismiss_finding(&self, ctx: &Context<'_>, id: String) -> Result<bool> {
        let writer = ctx.data_unchecked::<Arc<GraphStore>>();
        let dismissed = writer
            .dismiss_validation_issue(&id)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to dismiss finding: {e}")))?;
        Ok(dismissed)
    }

    /// Create a new scout task (manual demand signal). Geocodes the location server-side.
    #[graphql(guard = "AdminGuard")]
    async fn create_scout_task(
        &self,
        ctx: &Context<'_>,
        location: String,
        radius_km: Option<f64>,
        priority: Option<f64>,
    ) -> Result<String> {
        let (lat, lng, display_name) = geocode_location(&location)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Geocoding failed: {e}")))?;

        // Extract geo_terms from the display_name (comma-separated parts)
        let geo_terms: Vec<String> = display_name
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let writer = ctx.data_unchecked::<Arc<GraphStore>>();
        let task = rootsignal_common::ScoutTask {
            id: Uuid::new_v4(),
            center_lat: lat,
            center_lng: lng,
            radius_km: radius_km.unwrap_or(30.0),
            context: display_name,
            geo_terms,
            priority: priority.unwrap_or(1.0),
            source: rootsignal_common::ScoutTaskSource::Manual,
            status: rootsignal_common::ScoutTaskStatus::Pending,
            phase_status: "idle".to_string(),
            created_at: chrono::Utc::now(),
            completed_at: None,
        };
        let id = task.id.to_string();
        writer
            .upsert_scout_task(&task)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to create scout task: {e}")))?;
        Ok(id)
    }

    /// Cancel a scout task.
    #[graphql(guard = "AdminGuard")]
    async fn cancel_scout_task(&self, ctx: &Context<'_>, id: String) -> Result<bool> {
        let writer = ctx.data_unchecked::<Arc<GraphStore>>();
        let cancelled = writer
            .cancel_scout_task(&id)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to cancel scout task: {e}")))?;
        Ok(cancelled)
    }

    /// Record a demand signal from a user search (public, rate-limited).
    async fn record_demand(
        &self,
        ctx: &Context<'_>,
        query: String,
        center_lat: f64,
        center_lng: f64,
        radius_km: f64,
    ) -> Result<bool> {
        rate_limit_check(ctx, DEMAND_RATE_LIMIT_PER_HOUR)?;

        // Validate inputs
        let query = query.trim().to_string();
        if query.is_empty() || query.len() > 200 {
            return Err("Query must be 1-200 characters".into());
        }
        if !(-90.0..=90.0).contains(&center_lat) {
            return Err("center_lat must be between -90 and 90".into());
        }
        if !(-180.0..=180.0).contains(&center_lng) {
            return Err("center_lng must be between -180 and 180".into());
        }
        if !(1.0..=500.0).contains(&radius_km) {
            return Err("radius_km must be between 1 and 500".into());
        }

        let writer = ctx.data_unchecked::<Arc<GraphStore>>();
        let signal = DemandSignal {
            id: Uuid::new_v4(),
            query: query.clone(),
            center_lat,
            center_lng,
            radius_km,
            created_at: chrono::Utc::now(),
        };

        writer
            .upsert_demand_signal(&signal)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to record demand: {e}")))?;

        info!(query = query.as_str(), "Demand signal recorded");
        Ok(true)
    }

    /// Manually trigger a news scan (admin only).
    #[graphql(guard = "AdminGuard")]
    async fn run_news_scan(&self, ctx: &Context<'_>) -> Result<ScoutResult> {
        let config = ctx.data_unchecked::<Arc<Config>>();

        if config.anthropic_api_key.is_empty() || config.serper_api_key.is_empty() {
            return Ok(ScoutResult {
                success: false,
                message: Some("API keys not configured".to_string()),
            });
        }

        let restate = require_restate(ctx)?;
        restate
            .run_news_scan()
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to dispatch news scan: {e}")))?;

        Ok(ScoutResult {
            success: true,
            message: Some("News scan dispatched via Restate".into()),
        })
    }
}

fn rate_limit_check(ctx: &Context<'_>, max_per_hour: usize) -> Result<()> {
    let client_ip = ctx.data_unchecked::<ClientIp>();
    let limiter = ctx.data_unchecked::<RateLimiter>();

    // We need to block on the mutex - use try_lock to avoid async issues
    // In practice, contention is very low
    let mut guard = limiter
        .0
        .try_lock()
        .map_err(|_| async_graphql::Error::new("Rate limiter busy, try again"))?;

    let entries = guard.entry(client_ip.0).or_default();
    if !check_rate_limit_window(entries, Instant::now(), max_per_hour) {
        return Err(async_graphql::Error::new("Rate limit exceeded"));
    }

    Ok(())
}

fn check_rate_limit_window(entries: &mut Vec<Instant>, now: Instant, max_per_hour: usize) -> bool {
    let cutoff = now - std::time::Duration::from_secs(3600);
    entries.retain(|t| *t > cutoff);
    if entries.len() >= max_per_hour {
        return false;
    }
    entries.push(now);
    true
}

/// Create a per-mutation engine via the factory.
fn require_engine(ctx: &Context<'_>) -> Result<rootsignal_scout::core::engine::ScoutEngine> {
    ctx.data_unchecked::<Option<EngineFactory>>()
        .as_ref()
        .ok_or_else(|| async_graphql::Error::new("Engine not configured (Postgres required)"))
        .map(|f| f.create())
}

/// Extract the Restate client from GraphQL context, returning a clear error if not configured.
fn require_restate<'a>(ctx: &'a Context<'_>) -> Result<&'a RestateClient> {
    ctx.data_unchecked::<Option<RestateClient>>()
        .as_ref()
        .ok_or_else(|| {
            async_graphql::Error::new("Restate ingress not configured (set RESTATE_INGRESS_URL)")
        })
}

#[derive(serde::Deserialize)]
struct NominatimResult {
    lat: String,
    lon: String,
    display_name: String,
}

/// Geocode a location string to (lat, lng, display_name) using Nominatim.
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

#[cfg(test)]
mod tests {
    use super::*;
    use async_graphql::{EmptySubscription, Schema};
    use rootsignal_scout::store::{EngineFactory, SignalReaderFactory};
    use rootsignal_scout::testing::MockSignalReader;
    use rootsignal_scout::traits::SignalReader;
    use std::collections::HashMap;
    use std::net::{IpAddr, Ipv4Addr};

    use super::super::schema::QueryRoot;

    /// Build a test schema with MockSignalReader, engine factory, RateLimiter, and ClientIp.
    fn test_schema() -> Schema<QueryRoot, MutationRoot, EmptySubscription> {
        let store = Arc::new(MockSignalReader::new());
        let store_factory = SignalReaderFactory::fixed(store.clone() as Arc<dyn SignalReader>);
        let engine_factory = EngineFactory::fixed(store.clone() as Arc<dyn SignalReader>);
        Schema::build(QueryRoot, MutationRoot, EmptySubscription)
            .data(Some(store_factory))
            .data(Some(engine_factory))
            .data(RateLimiter(Mutex::new(HashMap::new())))
            .data(ClientIp(IpAddr::V4(Ipv4Addr::LOCALHOST)))
            .finish()
    }

    #[tokio::test]
    async fn valid_url_creates_source() {
        let schema = test_schema();
        let resp = schema
            .execute(r#"mutation { submitSource(url: "https://example.com/food-shelf") { success sourceId } }"#)
            .await;

        let data = resp.data.into_json().unwrap();
        assert_eq!(data["submitSource"]["success"], true);
        assert!(data["submitSource"]["sourceId"].as_str().is_some());
    }

    #[tokio::test]
    async fn invalid_url_is_rejected() {
        let schema = test_schema();
        let resp = schema
            .execute(r#"mutation { submitSource(url: "not-a-url") { success } }"#)
            .await;

        assert!(!resp.errors.is_empty());
        let msg = resp.errors[0].message.to_lowercase();
        assert!(
            msg.contains("invalid url"),
            "expected 'Invalid URL', got: {msg}"
        );
    }

    #[tokio::test]
    async fn ftp_scheme_is_rejected() {
        let schema = test_schema();
        let resp = schema
            .execute(r#"mutation { submitSource(url: "ftp://example.com") { success } }"#)
            .await;

        assert!(!resp.errors.is_empty());
        let msg = resp.errors[0].message.to_lowercase();
        assert!(
            msg.contains("http or https"),
            "expected scheme error, got: {msg}"
        );
    }

    #[tokio::test]
    async fn localhost_url_is_blocked() {
        let schema = test_schema();
        let resp = schema
            .execute(r#"mutation { submitSource(url: "https://localhost/admin") { success } }"#)
            .await;

        assert!(!resp.errors.is_empty());
        let msg = resp.errors[0].message.to_lowercase();
        assert!(
            msg.contains("internal hosts"),
            "expected internal hosts error, got: {msg}"
        );
    }

    #[tokio::test]
    async fn url_too_long_is_rejected() {
        let schema = test_schema();
        let long_url = format!("https://example.com/{}", "a".repeat(3000));
        let query = format!(
            r#"mutation {{ submitSource(url: "{}") {{ success }} }}"#,
            long_url
        );
        let resp = schema.execute(&query).await;

        assert!(!resp.errors.is_empty());
        let msg = resp.errors[0].message.to_lowercase();
        assert!(
            msg.contains("too long"),
            "expected 'too long' error, got: {msg}"
        );
    }
}

