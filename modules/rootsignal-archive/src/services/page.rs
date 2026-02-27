// Page service: web page fetching via Chrome or Browserless.
// Returns universal ArchivedPage content type.

use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rand::Rng;
use tracing::{info, warn};
use uuid::Uuid;

use crate::readability::html_to_markdown;
use crate::store::InsertPage;

/// Max concurrent Chromium processes. Each instance is heavy (~100MB+ RSS).
const MAX_CONCURRENT_CHROME: usize = 2;
/// Max retry attempts for transient Chrome failures.
const CHROME_MAX_ATTEMPTS: u32 = 3;
/// Base backoff duration for Chrome retries.
const CHROME_RETRY_BASE: Duration = Duration::from_secs(3);

pub(crate) struct FetchedPage {
    pub page: InsertPage,
    pub raw_html: String,
}

pub(crate) struct ChromePageService {
    semaphore: tokio::sync::Semaphore,
}

impl ChromePageService {
    pub(crate) fn new() -> Self {
        info!("ChromePageService initialized (max_concurrent={MAX_CONCURRENT_CHROME})");
        Self {
            semaphore: tokio::sync::Semaphore::new(MAX_CONCURRENT_CHROME),
        }
    }

    /// Fetch a web page via headless Chrome, returning an InsertPage.
    pub(crate) async fn fetch(&self, url: &str, source_id: Uuid) -> Result<FetchedPage> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| anyhow::anyhow!("Chrome semaphore closed"))?;

        info!(url, "page: fetching via chrome");

        let html_bytes = self.run_chrome(url).await?;

        if html_bytes.is_empty() {
            warn!(url, "page: empty DOM output");
            let hash = rootsignal_common::content_hash("").to_string();
            return Ok(FetchedPage {
                page: InsertPage {
                    source_id,
                    content_hash: hash,
                    markdown: String::new(),
                    title: None,
                    links: Vec::new(),
                },
                raw_html: String::new(),
            });
        }

        let raw_html = String::from_utf8_lossy(&html_bytes).into_owned();
        let markdown = html_to_markdown(&html_bytes, Some(url));
        let hash = rootsignal_common::content_hash(&raw_html).to_string();

        // Extract title from HTML
        let title = extract_title(&raw_html);

        info!(url, bytes = raw_html.len(), "page: fetched successfully");

        Ok(FetchedPage {
            page: InsertPage {
                source_id,
                content_hash: hash,
                markdown,
                title,
                links: Vec::new(),
            },
            raw_html,
        })
    }

    async fn run_chrome(&self, url: &str) -> Result<Vec<u8>> {
        let parsed = url::Url::parse(url).context("Invalid URL")?;
        if parsed.scheme() != "http" && parsed.scheme() != "https" {
            anyhow::bail!("Only http/https URLs allowed, got: {}", parsed.scheme());
        }

        let chrome_bin = std::env::var("CHROME_BIN").unwrap_or_else(|_| "chromium".to_string());

        for attempt in 0..CHROME_MAX_ATTEMPTS {
            let tmp_dir = tempfile::tempdir().context("Failed to create temp profile dir")?;

            let result = tokio::time::timeout(
                Duration::from_secs(30),
                tokio::process::Command::new(&chrome_bin)
                    .args([
                        "--headless",
                        "--no-sandbox",
                        "--disable-gpu",
                        "--disable-dev-shm-usage",
                        &format!("--user-data-dir={}", tmp_dir.path().display()),
                        "--dump-dom",
                        url,
                    ])
                    .output(),
            )
            .await;

            match result {
                Ok(Ok(output)) => {
                    if output.status.success() {
                        if output.stdout.is_empty() && attempt + 1 < CHROME_MAX_ATTEMPTS {
                            let backoff = CHROME_RETRY_BASE * 3u32.pow(attempt);
                            let jitter = Duration::from_millis(rand::rng().random_range(0..1000));
                            warn!(
                                url,
                                attempt = attempt + 1,
                                "Chrome returned empty DOM, retrying"
                            );
                            tokio::time::sleep(backoff + jitter).await;
                            continue;
                        }
                        return Ok(output.stdout);
                    }
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    if is_transient_error(&stderr) && attempt + 1 < CHROME_MAX_ATTEMPTS {
                        retry_with_backoff(url, attempt).await;
                        continue;
                    }
                    warn!(url, stderr = %stderr, "Chrome exited with error");
                    return Ok(Vec::new());
                }
                Ok(Err(e)) => {
                    let msg = e.to_string();
                    if is_transient_error(&msg) && attempt + 1 < CHROME_MAX_ATTEMPTS {
                        warn!(url, attempt = attempt + 1, error = %e, "Chrome launch failed, retrying");
                        retry_with_backoff(url, attempt).await;
                        continue;
                    }
                    anyhow::bail!("Failed to run Chrome for {url}: {e}");
                }
                Err(_) => {
                    if attempt + 1 < CHROME_MAX_ATTEMPTS {
                        warn!(url, attempt = attempt + 1, "Chrome timed out, retrying");
                        retry_with_backoff(url, attempt).await;
                        continue;
                    }
                    anyhow::bail!("Chrome timed out after 30s for {url}");
                }
            }
        }

        Ok(Vec::new())
    }
}

pub(crate) struct BrowserlessPageService {
    client: browserless_client::BrowserlessClient,
}

impl BrowserlessPageService {
    pub(crate) fn new(base_url: &str, token: Option<&str>) -> Self {
        info!(base_url, "BrowserlessPageService initialized");
        Self {
            client: browserless_client::BrowserlessClient::new(base_url, token),
        }
    }

    /// Fetch a page via Browserless.
    pub(crate) async fn fetch(&self, url: &str, source_id: Uuid) -> Result<FetchedPage> {
        info!(url, "page: fetching via browserless");

        let html = self
            .client
            .content(url)
            .await
            .context("Browserless content request failed")?;

        if html.is_empty() {
            warn!(url, "page: empty HTML response");
            let hash = rootsignal_common::content_hash("").to_string();
            return Ok(FetchedPage {
                page: InsertPage {
                    source_id,
                    content_hash: hash,
                    markdown: String::new(),
                    title: None,
                    links: Vec::new(),
                },
                raw_html: String::new(),
            });
        }

        let markdown = html_to_markdown(html.as_bytes(), Some(url));
        let hash = rootsignal_common::content_hash(&html).to_string();
        let title = extract_title(&html);

        info!(url, bytes = html.len(), "page: fetched successfully");

        Ok(FetchedPage {
            page: InsertPage {
                source_id,
                content_hash: hash,
                markdown,
                title,
                links: Vec::new(),
            },
            raw_html: html,
        })
    }
}

/// Extract links from raw HTML that match a given URL pattern.
/// Resolves relative URLs against `base_url`, deduplicates, and caps at 20 results.
pub fn extract_links_by_pattern(html: &str, base_url: &str, pattern: &str) -> Vec<String> {
    let href_re = regex::Regex::new(r#"href\s*=\s*["']([^"']+)["']"#).expect("valid regex");
    let base = url::Url::parse(base_url).ok();

    let mut seen = std::collections::HashSet::new();
    let mut links = Vec::new();

    for cap in href_re.captures_iter(html) {
        let raw = &cap[1];
        let resolved = if raw.starts_with("http://") || raw.starts_with("https://") {
            raw.to_string()
        } else if let Some(ref b) = base {
            match b.join(raw) {
                Ok(u) => u.to_string(),
                Err(_) => continue,
            }
        } else {
            continue;
        };

        if resolved.contains(pattern) && seen.insert(resolved.clone()) {
            links.push(resolved);
            if links.len() >= 20 {
                break;
            }
        }
    }

    links
}

/// Simple title extraction from HTML <title> tag.
pub(crate) fn extract_title(html: &str) -> Option<String> {
    let start = html.find("<title")?.checked_add(6)?;
    let rest = &html[start..];
    let tag_end = rest.find('>')?;
    let after_tag = &rest[tag_end + 1..];
    let end = after_tag.find("</title>")?;
    let title = after_tag[..end].trim();
    if title.is_empty() {
        None
    } else {
        Some(title.to_string())
    }
}

fn is_transient_error(msg: &str) -> bool {
    msg.contains("Cannot fork") || msg.contains("Resource temporarily unavailable")
}

async fn retry_with_backoff(_url: &str, attempt: u32) {
    let backoff = CHROME_RETRY_BASE * 3u32.pow(attempt);
    let jitter = Duration::from_millis(rand::rng().random_range(0..1000));
    tokio::time::sleep(backoff + jitter).await;
}

/// Extract a publication date from raw HTML metadata.
///
/// Priority order:
/// 1. JSON-LD `datePublished` / `dateModified`
/// 2. OpenGraph `article:published_time`
/// 3. Generic meta tags (`date`, `publish_date`, `pubdate`, `publish-date`, `DC.date.issued`)
/// 4. HTML5 `<time datetime="...">` element
///
/// Returns `None` if no parseable date is found.
pub fn extract_published_date(html: &str) -> Option<DateTime<Utc>> {
    // 1. JSON-LD
    if let Some(date) = extract_json_ld_date(html) {
        return Some(date);
    }

    // 2. OpenGraph article:published_time
    if let Some(date) = extract_meta_property(html, "article:published_time") {
        return Some(date);
    }

    // 3. Generic meta name tags
    for name in &[
        "date",
        "publish_date",
        "pubdate",
        "publish-date",
        "DC.date.issued",
    ] {
        if let Some(date) = extract_meta_name(html, name) {
            return Some(date);
        }
    }

    // 4. HTML5 <time datetime="...">
    extract_time_element(html)
}

/// Parse a date string into DateTime<Utc>, trying multiple formats.
fn parse_date(s: &str) -> Option<DateTime<Utc>> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    // ISO 8601 with timezone
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }

    // ISO 8601 without timezone (assume UTC)
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Some(dt.and_utc());
    }

    // Date only
    if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return d.and_hms_opt(0, 0, 0).map(|dt| dt.and_utc());
    }

    // US format: "Month Day, Year"
    if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%B %d, %Y") {
        return d.and_hms_opt(0, 0, 0).map(|dt| dt.and_utc());
    }

    None
}

fn extract_json_ld_date(html: &str) -> Option<DateTime<Utc>> {
    let script_re = regex::Regex::new(
        r#"(?si)<script[^>]*type\s*=\s*["']application/ld\+json["'][^>]*>(.*?)</script>"#,
    )
    .ok()?;

    for cap in script_re.captures_iter(html) {
        let json_str = &cap[1];
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(json_str) {
            // Try datePublished first, then dateModified
            for key in &["datePublished", "dateModified"] {
                if let Some(date_str) = value.get(key).and_then(|v| v.as_str()) {
                    if let Some(dt) = parse_date(date_str) {
                        return Some(dt);
                    }
                }
            }
            // Handle @graph arrays
            if let Some(graph) = value.get("@graph").and_then(|v| v.as_array()) {
                for item in graph {
                    for key in &["datePublished", "dateModified"] {
                        if let Some(date_str) = item.get(key).and_then(|v| v.as_str()) {
                            if let Some(dt) = parse_date(date_str) {
                                return Some(dt);
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

fn extract_meta_property(html: &str, property: &str) -> Option<DateTime<Utc>> {
    let pattern = format!(
        r#"(?i)<meta[^>]*property\s*=\s*["']{property}["'][^>]*content\s*=\s*["']([^"']+)["']"#
    );
    let re = regex::Regex::new(&pattern).ok()?;
    if let Some(cap) = re.captures(html) {
        return parse_date(&cap[1]);
    }
    // Also try content before property (attribute order varies)
    let pattern2 = format!(
        r#"(?i)<meta[^>]*content\s*=\s*["']([^"']+)["'][^>]*property\s*=\s*["']{property}["']"#
    );
    let re2 = regex::Regex::new(&pattern2).ok()?;
    if let Some(cap) = re2.captures(html) {
        return parse_date(&cap[1]);
    }
    None
}

fn extract_meta_name(html: &str, name: &str) -> Option<DateTime<Utc>> {
    let pattern =
        format!(r#"(?i)<meta[^>]*name\s*=\s*["']{name}["'][^>]*content\s*=\s*["']([^"']+)["']"#);
    let re = regex::Regex::new(&pattern).ok()?;
    if let Some(cap) = re.captures(html) {
        return parse_date(&cap[1]);
    }
    // Also try content before name
    let pattern2 =
        format!(r#"(?i)<meta[^>]*content\s*=\s*["']([^"']+)["'][^>]*name\s*=\s*["']{name}["']"#);
    let re2 = regex::Regex::new(&pattern2).ok()?;
    if let Some(cap) = re2.captures(html) {
        return parse_date(&cap[1]);
    }
    None
}

fn extract_time_element(html: &str) -> Option<DateTime<Utc>> {
    let re = regex::Regex::new(r#"(?i)<time[^>]*datetime\s*=\s*["']([^"']+)["']"#).ok()?;
    if let Some(cap) = re.captures(html) {
        return parse_date(&cap[1]);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_ld_date_published_extracts_date() {
        let html = r#"
            <html><head>
            <script type="application/ld+json">
            {"@type": "NewsArticle", "datePublished": "2025-06-15T10:30:00Z"}
            </script>
            </head><body></body></html>
        "#;
        let date = extract_published_date(html).unwrap();
        assert_eq!(date.format("%Y-%m-%d").to_string(), "2025-06-15");
    }

    #[test]
    fn json_ld_date_modified_used_as_fallback() {
        let html = r#"
            <html><head>
            <script type="application/ld+json">
            {"@type": "Article", "dateModified": "2025-07-01T08:00:00+00:00"}
            </script>
            </head><body></body></html>
        "#;
        let date = extract_published_date(html).unwrap();
        assert_eq!(date.format("%Y-%m-%d").to_string(), "2025-07-01");
    }

    #[test]
    fn json_ld_graph_array_extracts_date() {
        let html = r#"
            <html><head>
            <script type="application/ld+json">
            {"@graph": [{"@type": "WebPage", "datePublished": "2025-03-20T12:00:00Z"}]}
            </script>
            </head><body></body></html>
        "#;
        let date = extract_published_date(html).unwrap();
        assert_eq!(date.format("%Y-%m-%d").to_string(), "2025-03-20");
    }

    #[test]
    fn opengraph_article_published_time_extracts_date() {
        let html = r#"
            <html><head>
            <meta property="article:published_time" content="2025-05-10T14:00:00Z">
            </head><body></body></html>
        "#;
        let date = extract_published_date(html).unwrap();
        assert_eq!(date.format("%Y-%m-%d").to_string(), "2025-05-10");
    }

    #[test]
    fn meta_name_date_extracts_date() {
        let html = r#"
            <html><head>
            <meta name="date" content="2025-04-22">
            </head><body></body></html>
        "#;
        let date = extract_published_date(html).unwrap();
        assert_eq!(date.format("%Y-%m-%d").to_string(), "2025-04-22");
    }

    #[test]
    fn meta_name_publish_date_extracts_date() {
        let html = r#"
            <html><head>
            <meta name="publish_date" content="2025-08-01T09:00:00Z">
            </head><body></body></html>
        "#;
        let date = extract_published_date(html).unwrap();
        assert_eq!(date.format("%Y-%m-%d").to_string(), "2025-08-01");
    }

    #[test]
    fn html5_time_element_extracts_date() {
        let html = r#"
            <html><body>
            <article>
            <time datetime="2025-09-15T18:30:00Z">September 15, 2025</time>
            <p>Community event details...</p>
            </article>
            </body></html>
        "#;
        let date = extract_published_date(html).unwrap();
        assert_eq!(date.format("%Y-%m-%d").to_string(), "2025-09-15");
    }

    #[test]
    fn no_date_metadata_returns_none() {
        let html = r#"
            <html><head><title>No dates here</title></head>
            <body><p>Just some content.</p></body></html>
        "#;
        assert!(extract_published_date(html).is_none());
    }

    #[test]
    fn priority_order_json_ld_wins() {
        let html = r#"
            <html><head>
            <script type="application/ld+json">
            {"@type": "Article", "datePublished": "2025-01-01T00:00:00Z"}
            </script>
            <meta property="article:published_time" content="2025-06-01T00:00:00Z">
            <meta name="date" content="2025-12-01">
            </head>
            <body><time datetime="2025-09-01T00:00:00Z">Sep 1</time></body></html>
        "#;
        let date = extract_published_date(html).unwrap();
        assert_eq!(date.format("%Y-%m-%d").to_string(), "2025-01-01");
    }

    #[test]
    fn malformed_date_returns_none() {
        let html = r#"
            <html><head>
            <meta name="date" content="not-a-real-date">
            <meta property="article:published_time" content="garbage">
            </head><body><time datetime="also garbage">nope</time></body></html>
        "#;
        assert!(extract_published_date(html).is_none());
    }

    #[test]
    fn us_date_format_parses() {
        let html = r#"
            <html><head>
            <meta name="date" content="June 15, 2025">
            </head><body></body></html>
        "#;
        let date = extract_published_date(html).unwrap();
        assert_eq!(date.format("%Y-%m-%d").to_string(), "2025-06-15");
    }

    #[test]
    fn meta_attribute_order_reversed_still_works() {
        let html = r#"
            <html><head>
            <meta content="2025-11-20T10:00:00Z" property="article:published_time">
            </head><body></body></html>
        "#;
        let date = extract_published_date(html).unwrap();
        assert_eq!(date.format("%Y-%m-%d").to_string(), "2025-11-20");
    }
}
