use std::net::IpAddr;
use std::sync::Arc;
use std::time::Instant;

use async_graphql::{Context, Object, Result, SimpleObject};
use tokio::sync::Mutex;
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::{
    ActorNode, ActorType, Config, DemandSignal, DiscoveryMethod, ScoutScope, SourceNode, SourceRole,
    SubmissionNode,
};
use rootsignal_graph::GraphWriter;

use rootsignal_archive::Archive;

use rootsignal_scout::enrichment::actor_discovery::{
    create_actor_from_page, geocode_location,
};

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
struct ActorResult {
    success: bool,
    actor_id: Option<String>,
    location_name: Option<String>,
}

#[derive(SimpleObject)]
struct SubmitSourceResult {
    success: bool,
    source_id: Option<String>,
}

#[derive(SimpleObject)]
struct DiscoverActorsResult {
    discovered: u32,
    actors: Vec<ActorResult>,
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
        let writer = ctx.data_unchecked::<Arc<GraphWriter>>();
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

        writer
            .upsert_source(&source)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to create source: {e}")))?;

        info!(url, "Source added by admin");

        Ok(AddSourceResult {
            success: true,
            source_id: Some(source_id.to_string()),
        })
    }

    /// Run scout for a location query. Geocodes on backend, dispatches via Restate.
    #[graphql(guard = "AdminGuard")]
    async fn run_scout(&self, ctx: &Context<'_>, query: String) -> Result<ScoutResult> {
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

        let restate = ctx.data_unchecked::<Option<RestateClient>>();
        let restate = restate.as_ref().ok_or_else(|| {
            async_graphql::Error::new("Restate ingress not configured (set RESTATE_INGRESS_URL)")
        })?;

        // Geocode the query
        let (lat, lng, display_name) = geocode_location(&query)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Geocoding failed: {e}")))?;

        let slug = rootsignal_common::slugify(&query);

        let scope = ScoutScope {
            center_lat: lat,
            center_lng: lng,
            radius_km: 30.0,
            name: display_name.clone(),
            geo_terms: query.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
        };

        restate
            .run_scout(&slug, &scope)
            .await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;

        Ok(ScoutResult {
            success: true,
            message: Some(format!("Scout started via Restate for {display_name}")),
        })
    }

    /// Run an individual scout workflow phase for a location query.
    #[graphql(guard = "AdminGuard")]
    async fn run_scout_phase(
        &self,
        ctx: &Context<'_>,
        phase: super::types::ScoutPhase,
        query: String,
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

        let restate = ctx.data_unchecked::<Option<RestateClient>>();
        let restate = restate.as_ref().ok_or_else(|| {
            async_graphql::Error::new("Restate ingress not configured (set RESTATE_INGRESS_URL)")
        })?;

        let writer = ctx.data_unchecked::<Arc<GraphWriter>>();
        let restate_phase: crate::restate_client::ScoutPhase = phase.into();

        // Geocode the query
        let (lat, lng, display_name) = geocode_location(&query)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Geocoding failed: {e}")))?;

        let slug = rootsignal_common::slugify(&query);

        // Check prerequisites and atomically transition to running status
        let allowed = restate_phase.allowed_from_statuses();
        let running_status = restate_phase.running_status();

        if !writer
            .transition_region_status(&slug, allowed, running_status)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to check region status: {e}")))?
        {
            let current = writer
                .get_region_run_status(&slug)
                .await
                .unwrap_or_else(|_| "unknown".to_string());
            return Ok(ScoutResult {
                success: false,
                message: Some(format!(
                    "Cannot run {:?}: region status is '{current}'. Prerequisites not met or another phase is running.",
                    phase
                )),
            });
        }

        let scope = ScoutScope {
            center_lat: lat,
            center_lng: lng,
            radius_km: 30.0,
            name: display_name.clone(),
            geo_terms: query.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
        };

        restate
            .run_phase(restate_phase, &slug, &scope)
            .await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;

        Ok(ScoutResult {
            success: true,
            message: Some(format!("{:?} started via Restate for {display_name}", phase)),
        })
    }

    /// Stop a running scout workflow via Restate cancellation.
    #[graphql(guard = "AdminGuard")]
    async fn stop_scout(&self, ctx: &Context<'_>, query: String) -> Result<ScoutResult> {
        let restate = ctx.data_unchecked::<Option<RestateClient>>();
        let restate = restate.as_ref().ok_or_else(|| {
            async_graphql::Error::new("Restate ingress not configured (set RESTATE_INGRESS_URL)")
        })?;

        let slug = rootsignal_common::slugify(&query);

        match restate.cancel_scout(&slug).await {
            Ok(()) => Ok(ScoutResult {
                success: true,
                message: Some(format!("Cancel signal sent for {slug}")),
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

    /// Reset a stuck scout run status to idle.
    #[graphql(guard = "AdminGuard")]
    async fn reset_scout_status(&self, ctx: &Context<'_>, query: String) -> Result<ScoutResult> {
        let writer = ctx.data_unchecked::<Arc<GraphWriter>>();
        let slug = rootsignal_common::slugify(&query);
        info!(slug = slug.as_str(), "Scout status reset requested");
        writer
            .reset_region_run_status(&slug)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to reset status: {e}")))?;
        Ok(ScoutResult {
            success: true,
            message: Some("Status reset to idle".to_string()),
        })
    }

    /// Public source submission (rate-limited, no auth required).
    async fn submit_source(
        &self,
        ctx: &Context<'_>,
        url: String,
        description: Option<String>,
        region: Option<String>,
    ) -> Result<SubmitSourceResult> {
        let writer = ctx.data_unchecked::<Arc<GraphWriter>>();
        let config = ctx.data_unchecked::<Arc<Config>>();

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

        let region = region.unwrap_or_else(|| config.region.clone());
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
            gap_context: description.as_ref().map(|r| format!("Submission: {r}")),
            weight: 0.5,
            cadence_hours: None,
            avg_signals_per_scrape: 0.0,
            quality_penalty: 1.0,
            source_role: SourceRole::default(),
            scrape_count: 0,
        };

        writer
            .upsert_source(&source)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to create source: {e}")))?;

        // Create Submission node if reason provided
        if let Some(reason) = description.filter(|r| !r.trim().is_empty()) {
            let submission = SubmissionNode {
                id: Uuid::new_v4(),
                url: url.clone(),
                reason: Some(reason),
                region: region.clone(),
                submitted_at: now,
            };
            if let Err(e) = writer.upsert_submission(&submission, &canonical_key).await {
                warn!(error = %e, "Failed to create submission node");
            }
        }

        info!(url, region, "Human submission received via GraphQL");

        Ok(SubmitSourceResult {
            success: true,
            source_id: Some(source_id.to_string()),
        })
    }

    /// Add a curated tag to a story.
    #[graphql(guard = "AdminGuard")]
    async fn tag_story(
        &self,
        ctx: &Context<'_>,
        story_id: Uuid,
        tag_slug: String,
    ) -> Result<bool> {
        let writer = ctx.data_unchecked::<Arc<GraphWriter>>();
        let slug = rootsignal_common::slugify(&tag_slug);
        writer
            .batch_tag_signals(story_id, &[slug])
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to tag story: {e}")))?;
        Ok(true)
    }

    /// Remove a tag from a story (deletes TAGGED + creates SUPPRESSED_TAG).
    #[graphql(guard = "AdminGuard")]
    async fn untag_story(
        &self,
        ctx: &Context<'_>,
        story_id: Uuid,
        tag_slug: String,
    ) -> Result<bool> {
        let writer = ctx.data_unchecked::<Arc<GraphWriter>>();
        writer
            .suppress_story_tag(story_id, &tag_slug)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to untag story: {e}")))?;
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
        let writer = ctx.data_unchecked::<Arc<GraphWriter>>();
        writer
            .merge_tags(&source_slug, &target_slug)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to merge tags: {e}")))?;
        Ok(true)
    }

    /// Dismiss a supervisor finding (validation issue).
    #[graphql(guard = "AdminGuard")]
    async fn dismiss_finding(&self, ctx: &Context<'_>, id: String) -> Result<bool> {
        let writer = ctx.data_unchecked::<Arc<GraphWriter>>();
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

        let writer = ctx.data_unchecked::<Arc<GraphWriter>>();
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
        let writer = ctx.data_unchecked::<Arc<GraphWriter>>();
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

        let writer = ctx.data_unchecked::<Arc<GraphWriter>>();
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
        let graph_client = ctx.data_unchecked::<Arc<rootsignal_graph::GraphClient>>();

        if config.anthropic_api_key.is_empty() || config.serper_api_key.is_empty() {
            return Ok(ScoutResult {
                success: false,
                message: Some("API keys not configured".to_string()),
            });
        }

        let client = (**graph_client).clone();
        let config_clone = (**config).clone();

        tokio::spawn(async move {
            let writer = rootsignal_graph::GraphWriter::new(client.clone());
            let archive = match create_archive_for_news_scan(&config_clone).await {
                Some(archive) => archive,
                None => {
                    warn!("Cannot create archive for news scan (DATABASE_URL not set)");
                    return;
                }
            };
            let scanner = rootsignal_scout::news_scanner::NewsScanner::new(
                archive,
                &config_clone.anthropic_api_key,
                writer,
                config_clone.daily_budget_cents,
            );
            match scanner.scan().await {
                Ok(locations) => info!(count = locations.len(), "News scan complete"),
                Err(e) => warn!(error = %e, "News scan failed"),
            }
        });

        Ok(ScoutResult {
            success: true,
            message: Some("News scan started in background".to_string()),
        })
    }

    // ========== Actor mutations (AdminGuard) ==========

    /// Create an actor with a location string (geocoded on backend).
    /// Links social accounts if provided. The actor will be scraped on subsequent
    /// scout runs if it has a location and linked accounts.
    #[graphql(guard = "AdminGuard")]
    async fn create_actor(
        &self,
        ctx: &Context<'_>,
        name: String,
        #[graphql(desc = "organization | individual | government_body | coalition")]
        actor_type: Option<String>,
        #[graphql(desc = "Location string, e.g. 'Minneapolis, MN' — geocoded on backend")]
        location: String,
        bio: Option<String>,
        #[graphql(desc = "Social account URLs to link (e.g. https://instagram.com/handle)")]
        social_accounts: Option<Vec<String>>,
    ) -> Result<ActorResult> {
        let writer = ctx.data_unchecked::<Arc<GraphWriter>>();
        let name = name.trim().to_string();
        let location = location.trim().to_string();

        if name.is_empty() {
            return Err("Name is required".into());
        }
        if location.is_empty() {
            return Err("Location is required".into());
        }

        // Geocode location
        let (lat, lng, display_name) = geocode_location(&location)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Geocoding failed: {e}")))?;

        let actor_type = match actor_type.as_deref() {
            Some("individual") => ActorType::Individual,
            Some("government_body") => ActorType::GovernmentBody,
            Some("coalition") => ActorType::Coalition,
            _ => ActorType::Organization,
        };

        let entity_id = name.to_lowercase().replace(' ', "-");
        let actor = ActorNode {
            id: Uuid::new_v4(),
            name: name.clone(),
            actor_type,
            entity_id,
            domains: vec![],
            social_urls: social_accounts.clone().unwrap_or_default(),
            description: bio.clone().unwrap_or_default(),
            signal_count: 0,
            first_seen: chrono::Utc::now(),
            last_active: chrono::Utc::now(),
            typical_roles: vec![],
            bio,
            location_lat: Some(lat),
            location_lng: Some(lng),
            location_name: Some(display_name.clone()),
        };

        let actor_id = writer
            .upsert_actor_with_profile(&actor)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to create actor: {e}")))?;

        // Link social accounts as Source nodes with HAS_ACCOUNT edges
        for url in social_accounts.unwrap_or_default() {
            let url = url.trim().to_string();
            if url.is_empty() {
                continue;
            }
            let cv = rootsignal_common::canonical_value(&url);
            let source = SourceNode {
                id: Uuid::new_v4(),
                canonical_key: cv.clone(),
                canonical_value: cv.clone(),
                url: Some(url),
                discovery_method: DiscoveryMethod::ActorAccount,
                created_at: chrono::Utc::now(),
                last_scraped: None,
                last_produced_signal: None,
                signals_produced: 0,
                signals_corroborated: 0,
                consecutive_empty_runs: 0,
                active: true,
                gap_context: Some(format!("Actor account: {name}")),
                weight: 0.7,
                cadence_hours: Some(12),
                avg_signals_per_scrape: 0.0,
                quality_penalty: 1.0,
                source_role: SourceRole::Mixed,
                scrape_count: 0,
            };
            if let Err(e) = writer.upsert_source(&source).await {
                warn!(error = %e, "Failed to create actor source");
                continue;
            }
            if let Err(e) = writer.link_actor_account(actor_id, &cv).await {
                warn!(error = %e, "Failed to link actor account");
            }
        }

        info!(name = name.as_str(), location = display_name.as_str(), "Actor created");

        Ok(ActorResult {
            success: true,
            actor_id: Some(actor_id.to_string()),
            location_name: Some(display_name),
        })
    }

    /// Submit a URL and create an actor from it if the page represents one.
    /// Fetches the page, detects social links from HTML, uses LLM to extract actor identity.
    #[graphql(guard = "AdminGuard")]
    async fn submit_actor(
        &self,
        ctx: &Context<'_>,
        #[graphql(desc = "Any URL — org website, Linktree, about page")]
        url: String,
        #[graphql(desc = "Fallback location for geocoding if page doesn't mention one")]
        region: Option<String>,
    ) -> Result<ActorResult> {
        let config = ctx.data_unchecked::<Arc<Config>>();
        let writer = ctx.data_unchecked::<Arc<GraphWriter>>();

        if config.anthropic_api_key.is_empty() {
            return Err("Anthropic API key not configured".into());
        }

        let archive = create_archive_for_news_scan(&config)
            .await
            .ok_or_else(|| async_graphql::Error::new("Cannot create archive (DATABASE_URL not set)"))?;

        let fallback_region = region.unwrap_or_else(|| config.region.clone());

        match create_actor_from_page(
            &archive,
            &writer,
            &config.anthropic_api_key,
            &url,
            &fallback_region,
            false, // submit mode: don't require social links
        )
        .await
        {
            Ok(Some(result)) => Ok(ActorResult {
                success: true,
                actor_id: Some(result.actor_id.to_string()),
                location_name: Some(result.location_name),
            }),
            Ok(None) => Err("Page does not represent a specific actor".into()),
            Err(e) => Err(async_graphql::Error::new(format!("Failed to process page: {e}"))),
        }
    }

    /// Search the web and create actors from result pages that represent organizations/individuals.
    /// Skips pages without social links to avoid expensive LLM calls on non-actor pages.
    #[graphql(guard = "AdminGuard")]
    async fn discover_actors(
        &self,
        ctx: &Context<'_>,
        #[graphql(desc = "Web search query, e.g. 'mutual aid Minneapolis'")]
        query: String,
        #[graphql(desc = "Fallback location for all discovered actors")]
        region: String,
        #[graphql(desc = "Maximum search results to process (default 10)")]
        max_results: Option<i32>,
    ) -> Result<DiscoverActorsResult> {
        let config = ctx.data_unchecked::<Arc<Config>>();
        let writer = ctx.data_unchecked::<Arc<GraphWriter>>();

        if config.anthropic_api_key.is_empty() || config.serper_api_key.is_empty() {
            return Err("API keys not configured".into());
        }

        let archive = create_archive_for_news_scan(&config)
            .await
            .ok_or_else(|| async_graphql::Error::new("Cannot create archive (DATABASE_URL not set)"))?;

        let max = max_results.unwrap_or(10).min(50) as usize;

        // Fetch search results
        let handle = archive
            .source(&query)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Search failed: {e}")))?;
        let search = handle
            .search(&query)
            .max_results(max)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Search failed: {e}")))?;

        let urls: Vec<String> = search.results.into_iter().map(|r| r.url).collect();

        let mut actors = Vec::new();
        for url in &urls {
            match create_actor_from_page(
                &archive,
                &writer,
                &config.anthropic_api_key,
                url,
                &region,
                true, // discover mode: require social links
            )
            .await
            {
                Ok(Some(result)) => actors.push(ActorResult {
                    success: true,
                    actor_id: Some(result.actor_id.to_string()),
                    location_name: Some(result.location_name),
                }),
                Ok(None) => {} // not an actor page, skip
                Err(e) => warn!(url = url.as_str(), error = %e, "Failed to process search result"),
            }
        }

        let discovered = actors.len() as u32;
        info!(query = query.as_str(), region = region.as_str(), discovered, "Actor discovery complete");

        Ok(DiscoverActorsResult { discovered, actors })
    }

    /// Add a social account to an existing actor.
    #[graphql(guard = "AdminGuard")]
    async fn add_actor_account(
        &self,
        ctx: &Context<'_>,
        actor_id: String,
        url: String,
    ) -> Result<ActorResult> {
        let writer = ctx.data_unchecked::<Arc<GraphWriter>>();
        let url = url.trim().to_string();

        let actor_uuid = Uuid::parse_str(&actor_id)
            .map_err(|_| async_graphql::Error::new("Invalid actor ID"))?;

        let cv = rootsignal_common::canonical_value(&url);
        let source = SourceNode {
            id: Uuid::new_v4(),
            canonical_key: cv.clone(),
            canonical_value: cv.clone(),
            url: Some(url.clone()),
            discovery_method: DiscoveryMethod::ActorAccount,
            created_at: chrono::Utc::now(),
            last_scraped: None,
            last_produced_signal: None,
            signals_produced: 0,
            signals_corroborated: 0,
            consecutive_empty_runs: 0,
            active: true,
            gap_context: Some("Actor account".to_string()),
            weight: 0.7,
            cadence_hours: Some(12),
            avg_signals_per_scrape: 0.0,
            quality_penalty: 1.0,
            source_role: SourceRole::Mixed,
            scrape_count: 0,
        };

        writer
            .upsert_source(&source)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to create source: {e}")))?;

        writer
            .link_actor_account(actor_uuid, &cv)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to link account: {e}")))?;

        info!(actor_id = actor_id.as_str(), url = url.as_str(), "Actor account added");

        Ok(ActorResult {
            success: true,
            actor_id: Some(actor_id),
            location_name: None,
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

/// Create an Archive for news scanning. Returns None if DATABASE_URL is not set.
async fn create_archive_for_news_scan(
    config: &rootsignal_common::Config,
) -> Option<std::sync::Arc<Archive>> {
    let database_url = std::env::var("DATABASE_URL").ok()?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(3)
        .connect(&database_url)
        .await
        .ok()?;
    let archive_config = rootsignal_archive::ArchiveConfig {
        page_backend: match std::env::var("BROWSERLESS_URL") {
            Ok(url) => {
                let token = std::env::var("BROWSERLESS_TOKEN").ok();
                rootsignal_archive::PageBackend::Browserless { base_url: url, token }
            }
            Err(_) => rootsignal_archive::PageBackend::Chrome,
        },
        serper_api_key: config.serper_api_key.clone(),
        apify_api_key: if config.apify_api_key.is_empty() {
            None
        } else {
            Some(config.apify_api_key.clone())
        },
    };
    Some(std::sync::Arc::new(rootsignal_archive::Archive::new(
        pool,
        archive_config,
    )))
}

