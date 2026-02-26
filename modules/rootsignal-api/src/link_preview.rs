use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

const CACHE_TTL: Duration = Duration::from_secs(3600);
const MAX_CACHE_ENTRIES: usize = 500;
const FETCH_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_BODY_BYTES: usize = 100_000;
const HEAD_LIMIT: usize = 50_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkPreviewData {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site_name: Option<String>,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    data: LinkPreviewData,
    inserted_at: Instant,
}

const PREVIEW_RATE_LIMIT_PER_HOUR: usize = 30;

pub struct LinkPreviewCache {
    entries: RwLock<HashMap<String, CacheEntry>>,
    rate_limits: Mutex<HashMap<IpAddr, Vec<Instant>>>,
}

impl LinkPreviewCache {
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            rate_limits: Mutex::new(HashMap::new()),
        }
    }

    /// Returns true if the request is within rate limits, false if exceeded.
    async fn check_rate_limit(&self, ip: IpAddr) -> bool {
        let mut guard = self.rate_limits.lock().await;
        let entries = guard.entry(ip).or_default();
        let now = Instant::now();
        let cutoff = now - Duration::from_secs(3600);
        entries.retain(|t| *t > cutoff);
        if entries.is_empty() {
            // Will be re-populated below; if not, remove to prevent unbounded growth
        }
        if entries.len() >= PREVIEW_RATE_LIMIT_PER_HOUR {
            return false;
        }
        entries.push(now);
        true
    }

    async fn get(&self, url: &str) -> Option<LinkPreviewData> {
        let entries = self.entries.read().await;
        let entry = entries.get(url)?;
        if entry.inserted_at.elapsed() < CACHE_TTL {
            Some(entry.data.clone())
        } else {
            None
        }
    }

    async fn insert(&self, url: String, data: LinkPreviewData) {
        let mut entries = self.entries.write().await;
        // Opportunistic eviction when we hit the limit
        if entries.len() >= MAX_CACHE_ENTRIES {
            let now = Instant::now();
            entries.retain(|_, v| now.duration_since(v.inserted_at) < CACHE_TTL);
        }
        entries.insert(
            url,
            CacheEntry {
                data,
                inserted_at: Instant::now(),
            },
        );
    }
}

fn extract_og_tags(html: &str) -> LinkPreviewData {
    // Only look at the <head> section (or first HEAD_LIMIT bytes)
    let head = if let Some(end) = html[..html.len().min(HEAD_LIMIT)].find("</head>") {
        &html[..end]
    } else {
        &html[..html.len().min(HEAD_LIMIT)]
    };

    let og_re = Regex::new(
        r#"(?i)<meta\s+(?:[^>]*?\s)?(?:property|name)\s*=\s*["']og:(\w+)["'][^>]*?\scontent\s*=\s*["']([^"']*)["'][^>]*/?\s*>"#,
    )
    .unwrap();

    let og_rev_re = Regex::new(
        r#"(?i)<meta\s+(?:[^>]*?\s)?content\s*=\s*["']([^"']*)["'][^>]*?\s(?:property|name)\s*=\s*["']og:(\w+)["'][^>]*/?\s*>"#,
    )
    .unwrap();

    let mut title = None;
    let mut description = None;
    let mut image = None;
    let mut site_name = None;

    // property/name before content
    for cap in og_re.captures_iter(head) {
        let key = cap[1].to_lowercase();
        let value = cap[2].to_string();
        match key.as_str() {
            "title" if title.is_none() => title = Some(value),
            "description" if description.is_none() => description = Some(value),
            "image" if image.is_none() => image = Some(value),
            "site_name" if site_name.is_none() => site_name = Some(value),
            _ => {}
        }
    }

    // content before property/name
    for cap in og_rev_re.captures_iter(head) {
        let value = cap[1].to_string();
        let key = cap[2].to_lowercase();
        match key.as_str() {
            "title" if title.is_none() => title = Some(value),
            "description" if description.is_none() => description = Some(value),
            "image" if image.is_none() => image = Some(value),
            "site_name" if site_name.is_none() => site_name = Some(value),
            _ => {}
        }
    }

    // Fallback to <title> tag
    if title.is_none() {
        let title_re = Regex::new(r"(?i)<title[^>]*>([^<]+)</title>").unwrap();
        if let Some(cap) = title_re.captures(head) {
            title = Some(cap[1].trim().to_string());
        }
    }

    LinkPreviewData {
        url: String::new(), // filled in by handler
        title,
        description,
        image,
        site_name,
    }
}

#[derive(Deserialize)]
pub struct LinkPreviewQuery {
    url: String,
}

pub async fn link_preview_handler(
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    State(cache): State<Arc<LinkPreviewCache>>,
    Query(params): Query<LinkPreviewQuery>,
) -> impl IntoResponse {
    // Per-IP rate limiting
    if !cache.check_rate_limit(addr.ip()).await {
        return (StatusCode::TOO_MANY_REQUESTS, "Rate limit exceeded").into_response();
    }

    // Validate URL: scheme, length, SSRF protection
    let parsed = match rootsignal_common::validate_external_url(&params.url) {
        Ok(u) => u,
        Err(msg) => return (StatusCode::BAD_REQUEST, msg).into_response(),
    };

    let url_str = parsed.to_string();

    // Check cache
    if let Some(cached) = cache.get(&url_str).await {
        return Json(cached).into_response();
    }

    // Fetch
    let client = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let response = match client
        .get(&url_str)
        .header("User-Agent", "RootSignalBot/1.0 (link preview)")
        .send()
        .await
    {
        Ok(r) => r,
        Err(_) => {
            return (StatusCode::BAD_GATEWAY, "Failed to fetch URL").into_response();
        }
    };

    if !response.status().is_success() {
        return (StatusCode::BAD_GATEWAY, "Upstream returned error").into_response();
    }

    // Read body with size limit
    let body = match response.bytes().await {
        Ok(b) => b,
        Err(_) => {
            return (StatusCode::BAD_GATEWAY, "Failed to read response body").into_response();
        }
    };

    let body = if body.len() > MAX_BODY_BYTES {
        &body[..MAX_BODY_BYTES]
    } else {
        &body[..]
    };

    let html = String::from_utf8_lossy(body);
    let mut data = extract_og_tags(&html);
    data.url = url_str.clone();

    cache.insert(url_str, data.clone()).await;

    Json(data).into_response()
}
