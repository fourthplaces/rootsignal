use std::net::IpAddr;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

use async_graphql::{Context, Object, Result, SimpleObject};
use tokio::sync::Mutex;
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::{
    CityNode, Config, DiscoveryMethod, SourceNode, SourceRole, SubmissionNode,
};
use rootsignal_graph::GraphWriter;

use rootsignal_graph::cache::CacheStore;
use rootsignal_graph::cause_heat::compute_cause_heat;
use rootsignal_graph::GraphClient;
use rootsignal_scout::scout::Scout;

use crate::jwt::{self, JwtService};

use super::context::AdminGuard;

/// Rate limiter state shared via GraphQL context.
pub struct RateLimiter(pub Mutex<std::collections::HashMap<IpAddr, Vec<Instant>>>);

/// The client IP, extracted from the HTTP request and passed into GraphQL context.
pub struct ClientIp(pub IpAddr);

/// The scout cancel flag, shared with background scout threads.
pub struct ScoutCancel(pub Arc<std::sync::atomic::AtomicBool>);

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
struct CreateCityResult {
    success: bool,
    slug: Option<String>,
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

#[derive(SimpleObject)]
struct ResetRegionResult {
    success: bool,
    message: Option<String>,
    deleted_signals: u64,
    deleted_stories: u64,
    deleted_actors: u64,
    deleted_sources: u64,
}

/// Test phone number — only available in debug builds.
#[cfg(debug_assertions)]
const TEST_PHONE: Option<&str> = Some("+1234567890");
#[cfg(not(debug_assertions))]
const TEST_PHONE: Option<&str> = None;

const AUTH_RATE_LIMIT_PER_HOUR: usize = 10;
const SUBMIT_RATE_LIMIT_PER_HOUR: usize = 10;

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

    /// Create a new city by geocoding a location string.
    #[graphql(guard = "AdminGuard")]
    async fn create_city(&self, ctx: &Context<'_>, location: String) -> Result<CreateCityResult> {
        let location = location.trim().to_string();
        if location.is_empty() {
            return Err("Location cannot be empty".into());
        }

        let writer = ctx.data_unchecked::<Arc<GraphWriter>>();
        let config = ctx.data_unchecked::<Arc<Config>>();
        let graph_client = ctx.data_unchecked::<Arc<rootsignal_graph::GraphClient>>();

        // Geocode
        let (lat, lon, display_name) = geocode_location(&location)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Geocoding failed: {e}")))?;

        // Derive slug
        let slug: String = location
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-");

        let geo_terms: Vec<String> = location
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let city = CityNode {
            id: Uuid::new_v4(),
            name: display_name,
            slug: slug.clone(),
            center_lat: lat,
            center_lng: lon,
            radius_km: 30.0,
            geo_terms,
            active: true,
            created_at: chrono::Utc::now(),
            last_scout_completed_at: None,
        };

        writer
            .upsert_city(&city)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to create city: {e}")))?;

        // Run cold-start bootstrapper (non-fatal)
        let bootstrap_writer = GraphWriter::new((**graph_client).clone());
        let searcher = rootsignal_scout::scraper::SerperSearcher::new(&config.serper_api_key);
        let bootstrapper = rootsignal_scout::bootstrap::Bootstrapper::new(
            &bootstrap_writer,
            &searcher,
            &config.anthropic_api_key,
            city,
        );
        match bootstrapper.run().await {
            Ok(n) => info!(sources = n, slug = slug.as_str(), "Bootstrap complete"),
            Err(e) => warn!(error = %e, "Bootstrap failed (non-fatal)"),
        }

        Ok(CreateCityResult {
            success: true,
            slug: Some(slug),
        })
    }

    /// Add a source to a region.
    #[graphql(guard = "AdminGuard")]
    async fn add_source(
        &self,
        ctx: &Context<'_>,
        region_slug: String,
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
            .upsert_source(&source, &region_slug)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to create source: {e}")))?;

        info!(url, region = region_slug.as_str(), "Source added by admin");

        Ok(AddSourceResult {
            success: true,
            source_id: Some(source_id.to_string()),
        })
    }

    /// Run scout for a region. Returns immediately; scout runs in background.
    #[graphql(guard = "AdminGuard")]
    async fn run_scout(&self, ctx: &Context<'_>, region_slug: String) -> Result<ScoutResult> {
        let config = ctx.data_unchecked::<Arc<Config>>();
        let writer = ctx.data_unchecked::<Arc<GraphWriter>>();
        let graph_client = ctx.data_unchecked::<Arc<rootsignal_graph::GraphClient>>();
        let cancel = ctx.data_unchecked::<ScoutCancel>();

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

        // Check if already running
        if writer.is_scout_running(&region_slug).await.unwrap_or(false) {
            return Ok(ScoutResult {
                success: false,
                message: Some("Scout already running for this region".to_string()),
            });
        }

        let cache_store = ctx.data_unchecked::<Arc<CacheStore>>();
        spawn_scout_run(
            (**graph_client).clone(),
            (**config).clone(),
            region_slug,
            cancel.0.clone(),
            cache_store.clone(),
        );

        Ok(ScoutResult {
            success: true,
            message: Some("Scout started".to_string()),
        })
    }

    /// Stop the currently running scout.
    #[graphql(guard = "AdminGuard")]
    async fn stop_scout(&self, ctx: &Context<'_>, region_slug: String) -> Result<ScoutResult> {
        let cancel = ctx.data_unchecked::<ScoutCancel>();
        info!(region = region_slug.as_str(), "Scout stop requested");
        cancel.0.store(true, Ordering::Relaxed);
        Ok(ScoutResult {
            success: true,
            message: Some("Stop signal sent".to_string()),
        })
    }

    /// Reset a stuck scout lock.
    #[graphql(guard = "AdminGuard")]
    async fn reset_scout_lock(&self, ctx: &Context<'_>, region_slug: String) -> Result<ScoutResult> {
        let writer = ctx.data_unchecked::<Arc<GraphWriter>>();
        info!(region = region_slug.as_str(), "Scout lock reset requested");
        writer
            .release_scout_lock(&region_slug)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to release lock: {e}")))?;
        Ok(ScoutResult {
            success: true,
            message: Some("Lock released".to_string()),
        })
    }

    /// Reset all derived data for a region (signals, stories, actors, sources, locks).
    /// The next scout run will bootstrap from scratch.
    #[graphql(guard = "AdminGuard")]
    async fn reset_region(
        &self,
        ctx: &Context<'_>,
        region_slug: String,
    ) -> Result<ResetRegionResult> {
        let writer = ctx.data_unchecked::<Arc<GraphWriter>>();
        let cache_store = ctx.data_unchecked::<Arc<CacheStore>>();
        info!(region = region_slug.as_str(), "Region reset requested");

        let stats = writer
            .reset_region(&region_slug)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Region reset failed: {e}")))?;

        cache_store.reload(&ctx.data_unchecked::<Arc<GraphClient>>()).await;

        Ok(ResetRegionResult {
            success: true,
            message: Some(format!(
                "Reset complete: {} signals, {} stories, {} actors, {} sources deleted",
                stats.deleted_signals, stats.deleted_stories, stats.deleted_actors, stats.deleted_sources
            )),
            deleted_signals: stats.deleted_signals,
            deleted_stories: stats.deleted_stories,
            deleted_actors: stats.deleted_actors,
            deleted_sources: stats.deleted_sources,
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
            .upsert_source(&source, &region)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to create source: {e}")))?;

        // Create Submission node if reason provided
        if let Some(reason) = description.filter(|r| !r.trim().is_empty()) {
            let submission = SubmissionNode {
                id: Uuid::new_v4(),
                url: url.clone(),
                reason: Some(reason),
                city: region.clone(),
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

fn check_rate_limit_window(entries: &mut Vec<Instant>, now: Instant, max_per_hour: usize) -> bool {
    let cutoff = now - std::time::Duration::from_secs(3600);
    entries.retain(|t| *t > cutoff);
    if entries.len() >= max_per_hour {
        return false;
    }
    entries.push(now);
    true
}

/// Spawn a scout run in a dedicated thread. Returns immediately.
fn spawn_scout_run(
    client: GraphClient,
    config: rootsignal_common::Config,
    region_slug: String,
    cancel: Arc<std::sync::atomic::AtomicBool>,
    cache_store: Arc<CacheStore>,
) {
    use std::sync::atomic::Ordering;
    cancel.store(false, Ordering::Relaxed);

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
        rt.block_on(async move {
            if let Err(e) = run_scout(&client, &config, &region_slug, cancel).await {
                tracing::error!(error = %e, "Scout run failed");
            } else {
                cache_store.reload(&client).await;
            }
        });
    });
}

async fn run_scout(
    client: &GraphClient,
    config: &rootsignal_common::Config,
    region_slug: &str,
    cancel: Arc<std::sync::atomic::AtomicBool>,
) -> anyhow::Result<()> {
    let writer = rootsignal_graph::GraphWriter::new(client.clone());

    let region = writer
        .get_region(region_slug)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Region '{}' not found in graph", region_slug))?;

    info!(region = region_slug, "Scout run starting");

    // Save region geo bounds before moving region into Scout
    let lat_delta = region.radius_km / 111.0;
    let lng_delta = region.radius_km / (111.0 * region.center_lat.to_radians().cos());
    let min_lat = region.center_lat - lat_delta;
    let max_lat = region.center_lat + lat_delta;
    let min_lng = region.center_lng - lng_delta;
    let max_lng = region.center_lng + lng_delta;

    let scout = Scout::new(
        client.clone(),
        &config.anthropic_api_key,
        &config.voyage_api_key,
        &config.serper_api_key,
        &config.apify_api_key,
        region,
        config.daily_budget_cents,
        cancel,
    )?;

    let stats = scout.run().await?;
    info!("Scout run complete. {stats}");

    compute_cause_heat(client, 0.7, min_lat, max_lat, min_lng, max_lng).await?;

    writer.set_region_scout_completed(region_slug).await?;

    Ok(())
}

/// Start the background scout interval loop (called from main).
pub fn start_scout_interval(
    client: GraphClient,
    config: rootsignal_common::Config,
    interval_hours: u64,
    cache_store: Arc<CacheStore>,
) {
    use std::sync::atomic::AtomicBool;
    info!(interval_hours, "Starting multi-region scout interval loop");

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
        rt.block_on(async move {
            let writer = rootsignal_graph::GraphWriter::new(client.clone());
            loop {
                let regions = match writer.list_regions().await {
                    Ok(c) => c.into_iter().filter(|c| c.active).collect::<Vec<_>>(),
                    Err(e) => {
                        tracing::error!(error = %e, "Scout interval: failed to list regions");
                        Vec::new()
                    }
                };

                let num_regions = regions.len().max(1);

                for region in &regions {
                    match writer.is_scout_running(&region.slug).await {
                        Ok(true) => {
                            info!(region = region.slug.as_str(), "Scout interval: already running, skipping");
                            continue;
                        }
                        Err(e) => {
                            warn!(region = region.slug.as_str(), error = %e, "Scout interval: lock check failed, skipping");
                            continue;
                        }
                        Ok(false) => {}
                    }

                    info!(region = region.slug.as_str(), "Scout interval: starting run");
                    let cancel = Arc::new(AtomicBool::new(false));
                    if let Err(e) = run_scout(&client, &config, &region.slug, cancel).await {
                        tracing::error!(region = region.slug.as_str(), error = %e, "Scout interval run failed");
                    } else {
                        cache_store.reload(&client).await;
                    }
                    break;
                }

                let sleep_secs = ((interval_hours * 3600) / num_regions as u64).max(30 * 60);
                info!(sleep_minutes = sleep_secs / 60, "Scout interval: sleeping until next run");
                tokio::time::sleep(std::time::Duration::from_secs(sleep_secs)).await;
            }
        });
    });
}
