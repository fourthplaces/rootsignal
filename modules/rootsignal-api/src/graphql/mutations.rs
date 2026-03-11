use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Instant;

use async_graphql::{Context, InputObject, Object, Result, SimpleObject};
use tokio::sync::Mutex;
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::{
    Config, DiscoveryMethod, Region, ScoutScope, SourceNode, SourceRole,
};
use rootsignal_graph::{CachedReader, GraphStore};
use rootsignal_scout::store::{EngineFactory, SignalReaderFactory};
use crate::jwt::{self, JwtService};
use crate::scout_runner::ScoutRunner;
use super::context::AdminGuard;


/// Rate limiter state shared via GraphQL context.
pub struct RateLimiter(pub Mutex<std::collections::HashMap<IpAddr, Vec<Instant>>>);

/// The client IP, extracted from the HTTP request and passed into GraphQL context.
pub struct ClientIp(pub IpAddr);

/// HTTP response headers that mutations can set (e.g., Set-Cookie).
/// Wrapped in a Mutex so mutations can append headers.
pub struct ResponseHeaders(pub Mutex<Vec<(String, String)>>);

pub struct MutationRoot;

#[derive(InputObject)]
struct ChannelWeightInput {
    channel: String,
    value: f64,
}

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
        let input = url.trim().to_string();

        if input.is_empty() {
            return Err("Source value cannot be empty".into());
        }
        if input.len() > 2048 {
            return Err("Source value too long (max 2048 characters)".into());
        }

        let is_query = rootsignal_common::is_web_query(&input);

        // Validate URL scheme for non-query sources
        if !is_query {
            let parsed = url::Url::parse(&input).map_err(|_| async_graphql::Error::new("Invalid URL"))?;
            if parsed.scheme() != "http" && parsed.scheme() != "https" {
                return Err("URL must use http or https scheme".into());
            }
        }

        let cv = rootsignal_common::canonical_value(&input);
        let canonical_key = cv.clone();
        let source_id = Uuid::new_v4();
        let now = chrono::Utc::now();

        let source = SourceNode {
            id: source_id,
            canonical_key: canonical_key.clone(),
            canonical_value: cv,
            url: if is_query { None } else { Some(input.clone()) },
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
            sources_discovered: 0,
            discovered_from_key: None,
            channel_weights: rootsignal_common::ChannelWeights::default_for(
                &rootsignal_common::scraping_strategy(&input),
            ),
        };

        engine
            .emit(rootsignal_scout::domains::discovery::events::DiscoveryEvent::SourcesDiscovered {
                sources: vec![source],
                discovered_by: "admin".into(),
            })
            .settled()
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to create source: {e}")))?;

        info!(source = input, "Source added by admin");

        Ok(AddSourceResult {
            success: true,
            source_id: Some(source_id.to_string()),
        })
    }

    // --- Region CRUD ---

    /// Create a new region by name. Geocodes the location automatically.
    #[graphql(guard = "AdminGuard")]
    async fn create_region(
        &self,
        ctx: &Context<'_>,
        name: String,
    ) -> Result<super::types::GqlRegion> {
        let writer = ctx.data_unchecked::<Arc<GraphStore>>();

        let (lat, lng, _display_name) = geocode_location(&name)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Geocoding failed: {e}")))?;

        let geo_terms = vec![name.clone()];

        let region = rootsignal_common::Region {
            id: Uuid::new_v4(),
            name,
            center_lat: lat,
            center_lng: lng,
            radius_km: 20.0,
            geo_terms,
            is_leaf: true,
            created_at: chrono::Utc::now(),
        };

        writer
            .upsert_region(&region)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to create region: {e}")))?;

        Ok(super::types::GqlRegion::from_region(region))
    }

    /// Delete a region.
    #[graphql(guard = "AdminGuard")]
    async fn delete_region(&self, ctx: &Context<'_>, id: String) -> Result<bool> {
        let writer = ctx.data_unchecked::<Arc<GraphStore>>();
        writer
            .delete_region(&id)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to delete region: {e}")))
    }

    /// Add a WATCHES edge from a region to a source.
    #[graphql(guard = "AdminGuard")]
    async fn add_region_source(
        &self,
        ctx: &Context<'_>,
        region_id: String,
        source_id: String,
    ) -> Result<bool> {
        let writer = ctx.data_unchecked::<Arc<GraphStore>>();
        writer
            .add_region_source(&region_id, &source_id)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to add source: {e}")))?;
        Ok(true)
    }

    /// Remove a WATCHES edge from a region to a source.
    #[graphql(guard = "AdminGuard")]
    async fn remove_region_source(
        &self,
        ctx: &Context<'_>,
        region_id: String,
        source_id: String,
    ) -> Result<bool> {
        let writer = ctx.data_unchecked::<Arc<GraphStore>>();
        writer
            .remove_region_source(&region_id, &source_id)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to remove source: {e}")))?;
        Ok(true)
    }

    /// Nest a child region under a parent region (CONTAINS edge).
    #[graphql(guard = "AdminGuard")]
    async fn nest_region(
        &self,
        ctx: &Context<'_>,
        parent_id: String,
        child_id: String,
    ) -> Result<bool> {
        let writer = ctx.data_unchecked::<Arc<GraphStore>>();
        writer
            .nest_region(&parent_id, &child_id)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to nest region: {e}")))?;
        Ok(true)
    }

    // --- Flow-based scout operations ---

    /// Run a bootstrap flow for a region: discover sources.
    #[graphql(guard = "AdminGuard")]
    async fn run_bootstrap(&self, ctx: &Context<'_>, region_id: String) -> Result<ScoutResult> {
        let (runner, region) = load_region_for_flow(ctx, &region_id).await?;
        if let Some(pool) = ctx.data_unchecked::<Option<sqlx::PgPool>>() {
            if crate::db::scout_run::is_region_busy(pool, &region_id).await.unwrap_or(false) {
                return Ok(ScoutResult { success: false, message: Some(format!("Region {} is busy", region.name)) });
            }
        }
        let scope = ScoutScope::from(&region);
        runner.run_bootstrap(&region_id, &scope).await;
        Ok(ScoutResult {
            success: true,
            message: Some(format!("Bootstrap started for {}", region.name)),
        })
    }

    /// Run a scrape flow for a region: auto-bootstraps if no sources, then scrapes.
    #[graphql(guard = "AdminGuard")]
    async fn run_scrape(&self, ctx: &Context<'_>, region_id: String) -> Result<ScoutResult> {
        let (runner, region) = load_region_for_flow(ctx, &region_id).await?;
        if let Some(pool) = ctx.data_unchecked::<Option<sqlx::PgPool>>() {
            if crate::db::scout_run::is_region_busy(pool, &region_id).await.unwrap_or(false) {
                return Ok(ScoutResult { success: false, message: Some(format!("Region {} is busy", region.name)) });
            }
        }
        let scope = ScoutScope::from(&region);
        runner.run_scrape(&region_id, &scope).await;
        Ok(ScoutResult {
            success: true,
            message: Some(format!("Scrape started for {}", region.name)),
        })
    }

    /// Run a weave flow for a region: cross-signal synthesis.
    #[graphql(guard = "AdminGuard")]
    async fn run_weave(&self, ctx: &Context<'_>, region_id: String) -> Result<ScoutResult> {
        let (runner, region) = load_region_for_flow(ctx, &region_id).await?;
        if let Some(pool) = ctx.data_unchecked::<Option<sqlx::PgPool>>() {
            if crate::db::scout_run::is_region_busy(pool, &region_id).await.unwrap_or(false) {
                return Ok(ScoutResult { success: false, message: Some(format!("Region {} is busy", region.name)) });
            }
        }
        let scope = ScoutScope::from(&region);
        runner.run_weave(&region_id, &scope).await;
        Ok(ScoutResult {
            success: true,
            message: Some(format!("Weave started for {}", region.name)),
        })
    }

    /// Run a scout-source flow: scrape specific sources.
    #[graphql(guard = "AdminGuard")]
    async fn run_scout_source(
        &self,
        ctx: &Context<'_>,
        source_ids: Vec<String>,
    ) -> Result<ScoutResult> {
        if let Some(pool) = ctx.data_unchecked::<Option<sqlx::PgPool>>() {
            let mut busy_ids = Vec::new();
            for sid in &source_ids {
                if crate::db::scout_run::is_source_busy(pool, sid).await.unwrap_or(false) {
                    busy_ids.push(sid.clone());
                }
            }
            if busy_ids.len() == source_ids.len() {
                return Ok(ScoutResult { success: false, message: Some("All requested sources are busy".into()) });
            }
        }

        let runner = require_runner(ctx)?;
        let writer = ctx.data_unchecked::<Arc<GraphStore>>();

        // Load actual SourceNodes so the engine has them at scheduling time
        let uuids: Vec<Uuid> = source_ids.iter()
            .filter_map(|id| Uuid::parse_str(id).ok())
            .collect();
        let sources = writer.get_sources_by_ids(&uuids).await
            .map_err(|e| async_graphql::Error::new(format!("Failed to load sources: {e}")))?;
        if sources.is_empty() {
            return Ok(ScoutResult { success: false, message: Some("No valid sources found".into()) });
        }

        // Derive region from the first source's WATCHES edge (if any)
        let region = writer.get_region_for_source(&source_ids[0]).await
            .unwrap_or(None);

        runner.run_scout_source(&source_ids, sources, region).await;
        Ok(ScoutResult {
            success: true,
            message: Some(format!("Scout started for {} sources", source_ids.len())),
        })
    }

    /// Coalesce from a specific signal — seed a coalesce-only engine from this signal.
    #[graphql(guard = "AdminGuard")]
    async fn coalesce_signal(&self, ctx: &Context<'_>, signal_id: String) -> Result<ScoutResult> {
        let runner = require_runner(ctx)?;
        let writer = ctx.data_unchecked::<Arc<GraphStore>>();
        let signal_uuid = Uuid::parse_str(&signal_id)
            .map_err(|_| async_graphql::Error::new("Invalid signal ID"))?;

        let region = writer
            .get_region_for_signal(&signal_id)
            .await
            .unwrap_or(None);

        let (region_id, scope) = match region {
            Some(r) => {
                let rid = r.id.to_string();
                let scope = ScoutScope::from(&r);
                (rid, scope)
            }
            None => {
                return Ok(ScoutResult {
                    success: false,
                    message: Some("Signal has no region".into()),
                })
            }
        };

        runner
            .run_coalesce_signal(&region_id, &scope, signal_uuid)
            .await;

        Ok(ScoutResult {
            success: true,
            message: Some("Coalescing started".into()),
        })
    }

    /// Cancel a running run by run_id.
    #[graphql(guard = "AdminGuard")]
    async fn cancel_run(&self, ctx: &Context<'_>, run_id: String) -> Result<ScoutResult> {
        let runner = require_runner(ctx)?;
        if runner.cancel_run(&run_id).await {
            Ok(ScoutResult {
                success: true,
                message: Some(format!("Cancel signal sent for run {run_id}")),
            })
        } else {
            Ok(ScoutResult {
                success: false,
                message: Some(format!("No active run found for {run_id}")),
            })
        }
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
            sources_discovered: 0,
            discovered_from_key: None,
            channel_weights: rootsignal_common::ChannelWeights::default_for(
                &rootsignal_common::scraping_strategy(&url),
            ),
        };

        engine
            .emit(rootsignal_scout::domains::discovery::events::DiscoveryEvent::SourcesDiscovered {
                sources: vec![source],
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
        use rootsignal_common::events::SystemEvent;

        let engine = require_engine(ctx)?;
        engine
            .emit(SystemEvent::TagSuppressed {
                situation_id,
                tag_slug,
            })
            .settled()
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
        use rootsignal_common::events::SystemEvent;

        let engine = require_engine(ctx)?;
        engine
            .emit(SystemEvent::TagsMerged {
                source_slug,
                target_slug,
            })
            .settled()
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to merge tags: {e}")))?;
        Ok(true)
    }

    /// Dismiss a supervisor finding (validation issue).
    #[graphql(guard = "AdminGuard")]
    async fn dismiss_finding(&self, ctx: &Context<'_>, id: String) -> Result<bool> {
        use rootsignal_common::events::SystemEvent;

        let engine = require_engine(ctx)?;
        engine
            .emit(SystemEvent::ValidationIssueDismissed {
                issue_id: id,
            })
            .settled()
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to dismiss finding: {e}")))?;
        Ok(true)
    }

    /// Update source properties (active, weight, quality_penalty). Flows through events.
    #[graphql(guard = "AdminGuard")]
    async fn update_source(
        &self,
        ctx: &Context<'_>,
        id: Uuid,
        active: Option<bool>,
        weight: Option<f64>,
        quality_penalty: Option<f64>,
        channel_weights: Option<Vec<ChannelWeightInput>>,
    ) -> Result<ScoutResult> {
        use rootsignal_common::events::{SourceChange, SystemSourceChange};
        use rootsignal_common::events::SystemEvent;

        let writer = ctx.data_unchecked::<Arc<GraphStore>>();
        let sources = writer
            .get_sources_by_ids(&[id])
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to load source: {e}")))?;
        let source = sources
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new(format!("Source {id} not found")))?;

        let engine = require_engine(ctx)?;

        if let Some(new_active) = active {
            if new_active != source.active {
                engine
                    .emit(SystemEvent::SourceChanged {
                        source_id: source.id,
                        canonical_key: source.canonical_key.clone(),
                        change: SourceChange::Active {
                            old: source.active,
                            new: new_active,
                        },
                    })
                    .settled()
                    .await
                    .map_err(|e| async_graphql::Error::new(format!("Failed to update active: {e}")))?;
            }
        }

        if let Some(new_weight) = weight {
            if (new_weight - source.weight).abs() > f64::EPSILON {
                engine
                    .emit(SystemEvent::SourceChanged {
                        source_id: source.id,
                        canonical_key: source.canonical_key.clone(),
                        change: SourceChange::Weight {
                            old: source.weight,
                            new: new_weight,
                        },
                    })
                    .settled()
                    .await
                    .map_err(|e| async_graphql::Error::new(format!("Failed to update weight: {e}")))?;
            }
        }

        if let Some(new_qp) = quality_penalty {
            if (new_qp - source.quality_penalty).abs() > f64::EPSILON {
                engine
                    .emit(SystemEvent::SourceSystemChanged {
                        source_id: source.id,
                        canonical_key: source.canonical_key.clone(),
                        change: SystemSourceChange::QualityPenalty {
                            old: source.quality_penalty,
                            new: new_qp,
                        },
                    })
                    .settled()
                    .await
                    .map_err(|e| async_graphql::Error::new(format!("Failed to update quality_penalty: {e}")))?;
            }
        }

        if let Some(updates) = channel_weights {
            for cw in updates {
                let old = source.channel_weights.get(&cw.channel);
                if (cw.value - old).abs() > f64::EPSILON {
                    engine
                        .emit(SystemEvent::SourceChanged {
                            source_id: source.id,
                            canonical_key: source.canonical_key.clone(),
                            change: SourceChange::ChannelWeight {
                                channel: cw.channel,
                                old,
                                new: cw.value,
                            },
                        })
                        .settled()
                        .await
                        .map_err(|e| async_graphql::Error::new(format!("Failed to update channel weight: {e}")))?;
                }
            }
        }

        Ok(ScoutResult {
            success: true,
            message: Some(format!("Source {} updated", source.canonical_key)),
        })
    }

    /// Clear all signals produced by a source. Flows through events.
    #[graphql(guard = "AdminGuard")]
    async fn clear_source_signals(&self, ctx: &Context<'_>, source_id: Uuid) -> Result<ScoutResult> {
        use rootsignal_common::events::SystemEvent;

        let writer = ctx.data_unchecked::<Arc<GraphStore>>();
        let sources = writer
            .get_sources_by_ids(&[source_id])
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to load source: {e}")))?;
        let source = sources
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new(format!("Source {source_id} not found")))?;

        let engine = require_engine(ctx)?;
        engine
            .emit(SystemEvent::SourceSignalsCleared {
                source_id: source.id,
                canonical_key: source.canonical_key.clone(),
            })
            .settled()
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to clear signals: {e}")))?;

        Ok(ScoutResult {
            success: true,
            message: Some(format!("Signals cleared for {}", source.canonical_key)),
        })
    }

    /// Delete a source and all its edges. Flows through events.
    #[graphql(guard = "AdminGuard")]
    async fn delete_source(&self, ctx: &Context<'_>, id: Uuid) -> Result<ScoutResult> {
        use rootsignal_common::events::SystemEvent;

        let writer = ctx.data_unchecked::<Arc<GraphStore>>();
        let sources = writer
            .get_sources_by_ids(&[id])
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to load source: {e}")))?;
        let source = sources
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new(format!("Source {id} not found")))?;

        let engine = require_engine(ctx)?;
        engine
            .emit(SystemEvent::SourceDeleted {
                source_id: source.id,
                canonical_key: source.canonical_key.clone(),
            })
            .settled()
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to delete source: {e}")))?;

        Ok(ScoutResult {
            success: true,
            message: Some(format!("Source {} deleted", source.canonical_key)),
        })
    }

    /// Delete an actor and all its edges.
    #[graphql(guard = "AdminGuard")]
    async fn delete_actor(&self, ctx: &Context<'_>, id: Uuid) -> Result<ScoutResult> {
        use rootsignal_common::events::SystemEvent;

        let reader = ctx.data_unchecked::<Arc<CachedReader>>();
        let actor = reader
            .actor_detail(id)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to load actor: {e}")))?
            .ok_or_else(|| async_graphql::Error::new(format!("Actor {id} not found")))?;

        let engine = require_engine(ctx)?;
        engine
            .emit(SystemEvent::OrphanedActorsCleaned {
                actor_ids: vec![id],
            })
            .settled()
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to delete actor: {e}")))?;

        Ok(ScoutResult {
            success: true,
            message: Some(format!("Actor '{}' deleted", actor.name)),
        })
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
        use rootsignal_common::events::SystemEvent;

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

        let demand_id = Uuid::new_v4();
        let engine = require_engine(ctx)?;
        engine
            .emit(SystemEvent::DemandReceived {
                demand_id,
                query: query.clone(),
                center_lat,
                center_lng,
                radius_km,
            })
            .settled()
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to record demand: {e}")))?;

        info!(query = query.as_str(), "Demand signal recorded");
        Ok(true)
    }

    /// Set the budget config: daily spend ceiling and per-run cap. 0 = unlimited.
    #[graphql(guard = "AdminGuard")]
    async fn set_budget(
        &self,
        ctx: &Context<'_>,
        daily_limit_cents: i64,
        per_run_max_cents: i64,
    ) -> Result<bool> {
        if daily_limit_cents < 0 {
            return Err("daily_limit_cents must be non-negative".into());
        }
        if per_run_max_cents < 0 {
            return Err("per_run_max_cents must be non-negative".into());
        }

        let pool = ctx.data_unchecked::<Option<sqlx::PgPool>>();
        let pool = pool
            .as_ref()
            .ok_or_else(|| async_graphql::Error::new("Postgres not configured"))?;

        crate::db::models::budget::set_config(pool, daily_limit_cents, per_run_max_cents)
            .await
            .map_err(|e| async_graphql::Error::new(format!("Failed to set budget: {e}")))?;

        info!(daily_limit_cents, per_run_max_cents, "Budget updated");
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

        let runner = require_runner(ctx)?;
        runner.run_news_scan().await;

        Ok(ScoutResult {
            success: true,
            message: Some("News scan dispatched".into()),
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

/// Extract the scout runner from GraphQL context.
fn require_runner<'a>(ctx: &'a Context<'_>) -> Result<&'a ScoutRunner> {
    ctx.data_unchecked::<Option<ScoutRunner>>()
        .as_ref()
        .ok_or_else(|| {
            async_graphql::Error::new("Scout runner not configured (Postgres required)")
        })
}

/// Load a region and runner for flow mutations. Checks API keys.
async fn load_region_for_flow<'a>(
    ctx: &'a Context<'_>,
    region_id: &str,
) -> Result<(&'a ScoutRunner, Region)> {
    let config = ctx.data_unchecked::<Arc<Config>>();
    if config.anthropic_api_key.is_empty()
        || config.voyage_api_key.is_empty()
        || config.serper_api_key.is_empty()
    {
        return Err(async_graphql::Error::new("Scout API keys not configured"));
    }

    let runner = require_runner(ctx)?;
    let writer = ctx.data_unchecked::<Arc<GraphStore>>();

    let region = writer
        .get_region(region_id)
        .await
        .map_err(|e| async_graphql::Error::new(format!("Failed to load region: {e}")))?
        .ok_or_else(|| async_graphql::Error::new(format!("Region {region_id} not found")))?;

    Ok((runner, region))
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

