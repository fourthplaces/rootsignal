use std::sync::Arc;
use std::time::Instant;

use axum::{
    extract::{ConnectInfo, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde::Deserialize;
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::{DiscoveryMethod, SourceNode, SourceType, SubmissionNode};

use crate::AppState;

#[derive(Deserialize)]
pub struct SubmitRequest {
    url: String,
    reason: Option<String>,
    city: Option<String>,
}

pub const RATE_LIMIT_PER_HOUR: usize = 10;

/// Check rate limit for an IP. Returns true if the request is allowed, false if rate-limited.
/// Prunes expired entries and records the new request if allowed.
pub fn check_rate_limit(entries: &mut Vec<Instant>, now: Instant, max_per_hour: usize) -> bool {
    let cutoff = now - std::time::Duration::from_secs(3600);
    entries.retain(|t| *t > cutoff);
    if entries.len() >= max_per_hour {
        return false;
    }
    entries.push(now);
    true
}

pub async fn api_submit(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    Json(body): Json<SubmitRequest>,
) -> impl IntoResponse {
    // Validate URL: parse, enforce scheme, reject private/loopback IPs, enforce max length
    let url = body.url.trim().to_string();
    if url.len() > 2048 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "URL too long (max 2048 characters)"})),
        )
            .into_response();
    }
    let parsed_url = match url::Url::parse(&url) {
        Ok(u) => u,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid URL"})),
            )
                .into_response();
        }
    };
    if parsed_url.scheme() != "http" && parsed_url.scheme() != "https" {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "URL must use http or https scheme"})),
        )
            .into_response();
    }
    // Block private/loopback IPs to prevent SSRF
    if let Some(host) = parsed_url.host_str() {
        if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            if ip.is_loopback() || is_private_ip(ip) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "URLs pointing to private/loopback addresses are not allowed"})),
                )
                    .into_response();
            }
        }
        // Block common internal hostnames
        let lower = host.to_lowercase();
        if lower == "localhost" || lower.ends_with(".local") || lower.ends_with(".internal") {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "URLs pointing to internal hosts are not allowed"})),
            )
                .into_response();
        }
    }

    // Rate limit: 10 submissions per hour per IP
    let ip = addr.ip();
    {
        let mut limiter = state.rate_limiter.lock().await;
        // Periodically prune empty entries to prevent unbounded HashMap growth
        if limiter.len() > 1000 {
            prune_empty_entries(&mut limiter);
        }
        let entries = limiter.entry(ip).or_default();
        if !check_rate_limit(entries, Instant::now(), RATE_LIMIT_PER_HOUR) {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({"error": "Rate limit exceeded — max 10 submissions per hour"})),
            )
                .into_response();
        }
    }

    let city = body.city.as_deref().unwrap_or(&state.city).to_string();
    let source_type = SourceType::from_url(&url);
    let canonical_value = canonical_value_from_url(source_type, &url);
    let canonical_key = format!("{}:{}:{}", city, source_type, canonical_value);

    let now = chrono::Utc::now();
    let source_id = Uuid::new_v4();
    let source = SourceNode {
        id: source_id,
        canonical_key: canonical_key.clone(),
        canonical_value,
        url: Some(url.clone()),
        source_type,
        discovery_method: DiscoveryMethod::HumanSubmission,
        city: city.clone(),
        created_at: now,
        last_scraped: None,
        last_produced_signal: None,
        signals_produced: 0,
        signals_corroborated: 0,
        consecutive_empty_runs: 0,
        active: true,
        gap_context: body.reason.clone().map(|r| format!("Submission: {r}")),
        weight: 0.5,
        cadence_hours: None,
        avg_signals_per_scrape: 0.0,
        total_cost_cents: 0,
        last_cost_cents: 0,
        taxonomy_stats: None,
        quality_penalty: 1.0,
    };

    if let Err(e) = state.writer.upsert_source(&source).await {
        warn!(error = %e, "Failed to create submitted source");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    // If reason is non-empty, create a Submission node for investigation
    let reason = body.reason.filter(|r| !r.trim().is_empty());
    if reason.is_some() {
        let submission = SubmissionNode {
            id: Uuid::new_v4(),
            url: url.clone(),
            reason: reason.clone(),
            city: city.clone(),
            submitted_at: now,
        };
        if let Err(e) = state.writer.upsert_submission(&submission, &canonical_key).await {
            warn!(error = %e, "Failed to create submission node");
            // Source was created; submission linkage is non-critical
        }
    }

    // Log submission without verbatim reason (may contain PII)
    info!(url, city, has_reason = reason.is_some(), "Human submission received");

    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({
            "status": "accepted",
            "source_id": source_id.to_string(),
        })),
    )
        .into_response()
}

/// Check if an IP address is in a private range (RFC 1918 / RFC 4193).
fn is_private_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_link_local()
                || v4.octets()[0] == 10
                || (v4.octets()[0] == 172 && (16..=31).contains(&v4.octets()[1]))
                || (v4.octets()[0] == 192 && v4.octets()[1] == 168)
                || (v4.octets()[0] == 169 && v4.octets()[1] == 254) // metadata endpoint
        }
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback() || (v6.segments()[0] & 0xfe00) == 0xfc00
        }
    }
}

/// Prune empty entries from the rate limiter HashMap to prevent unbounded growth.
pub fn prune_empty_entries(limiter: &mut std::collections::HashMap<std::net::IpAddr, Vec<Instant>>) {
    let cutoff = Instant::now() - std::time::Duration::from_secs(3600);
    limiter.retain(|_, entries| {
        entries.retain(|t| *t > cutoff);
        !entries.is_empty()
    });
}

/// Extract the canonical value from a URL for deduplication.
pub fn canonical_value_from_url(source_type: SourceType, url: &str) -> String {
    match source_type {
        SourceType::Instagram => {
            // https://www.instagram.com/{username}/ → username
            url.split("instagram.com/")
                .nth(1)
                .unwrap_or(url)
                .trim_matches('/')
                .split('/')
                .next()
                .unwrap_or(url)
                .to_lowercase()
        }
        SourceType::Reddit => {
            // https://reddit.com/r/{subreddit} → subreddit
            if let Some(rest) = url.split("/r/").nth(1) {
                rest.trim_matches('/')
                    .split('/')
                    .next()
                    .unwrap_or(url)
                    .to_lowercase()
            } else {
                url.to_lowercase()
            }
        }
        _ => url.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- infer_source_type tests ---

    #[test]
    fn infer_instagram() {
        assert_eq!(SourceType::from_url("https://www.instagram.com/mpls_mutual_aid"), SourceType::Instagram);
    }

    #[test]
    fn infer_facebook() {
        assert_eq!(SourceType::from_url("https://facebook.com/somepage"), SourceType::Facebook);
    }

    #[test]
    fn infer_reddit() {
        assert_eq!(SourceType::from_url("https://reddit.com/r/Minneapolis"), SourceType::Reddit);
    }

    #[test]
    fn infer_tiktok() {
        assert_eq!(SourceType::from_url("https://www.tiktok.com/@someuser"), SourceType::TikTok);
    }

    #[test]
    fn infer_twitter() {
        assert_eq!(SourceType::from_url("https://twitter.com/user"), SourceType::Twitter);
    }

    #[test]
    fn infer_x_dot_com() {
        assert_eq!(SourceType::from_url("https://x.com/user"), SourceType::Twitter);
    }

    #[test]
    fn infer_bluesky() {
        assert_eq!(SourceType::from_url("https://bsky.app/profile/someone"), SourceType::Bluesky);
    }

    #[test]
    fn infer_plain_web() {
        assert_eq!(SourceType::from_url("https://www.startribune.com/article"), SourceType::Web);
    }

    // --- canonical_value_from_url tests ---

    #[test]
    fn canonical_instagram_username() {
        let val = canonical_value_from_url(SourceType::Instagram, "https://www.instagram.com/MplsMutualAid/");
        assert_eq!(val, "mplsmutualaid");
    }

    #[test]
    fn canonical_instagram_with_path() {
        let val = canonical_value_from_url(SourceType::Instagram, "https://instagram.com/user123/reels");
        assert_eq!(val, "user123");
    }

    #[test]
    fn canonical_reddit_subreddit() {
        let val = canonical_value_from_url(SourceType::Reddit, "https://reddit.com/r/Minneapolis/");
        assert_eq!(val, "minneapolis");
    }

    #[test]
    fn canonical_reddit_with_post_path() {
        let val = canonical_value_from_url(SourceType::Reddit, "https://www.reddit.com/r/TwinCities/comments/abc123");
        assert_eq!(val, "twincities");
    }

    #[test]
    fn canonical_web_returns_full_url() {
        let url = "https://www.startribune.com/some-article";
        let val = canonical_value_from_url(SourceType::Web, url);
        assert_eq!(val, url);
    }

    // --- rate limiter tests ---

    #[test]
    fn rate_limit_allows_under_limit() {
        let mut entries = Vec::new();
        let now = Instant::now();
        for _ in 0..9 {
            assert!(check_rate_limit(&mut entries, now, 10));
        }
        assert_eq!(entries.len(), 9);
    }

    #[test]
    fn rate_limit_allows_exactly_at_limit() {
        let mut entries = Vec::new();
        let now = Instant::now();
        for _ in 0..10 {
            assert!(check_rate_limit(&mut entries, now, 10));
        }
        assert_eq!(entries.len(), 10);
    }

    #[test]
    fn rate_limit_rejects_over_limit() {
        let mut entries = Vec::new();
        let now = Instant::now();
        for _ in 0..10 {
            assert!(check_rate_limit(&mut entries, now, 10));
        }
        // 11th should be rejected
        assert!(!check_rate_limit(&mut entries, now, 10));
        // entries should not grow past 10
        assert_eq!(entries.len(), 10);
    }

    #[test]
    fn rate_limit_expires_old_entries() {
        let mut entries = Vec::new();
        let old = Instant::now() - std::time::Duration::from_secs(3601);
        // Simulate 10 old entries
        for _ in 0..10 {
            entries.push(old);
        }
        // New request should be allowed because old ones expired
        let now = Instant::now();
        assert!(check_rate_limit(&mut entries, now, 10));
        // Old entries should have been pruned
        assert_eq!(entries.len(), 1);
    }
}
