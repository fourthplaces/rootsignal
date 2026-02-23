// Page service: web page fetching via Chrome or Browserless.
// Returns universal ArchivedPage content type.

use std::time::Duration;

use anyhow::{Context, Result};
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
    pub(crate) async fn fetch(
        &self,
        url: &str,
        source_id: Uuid,
    ) -> Result<FetchedPage> {
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
                            let jitter =
                                Duration::from_millis(rand::rng().random_range(0..1000));
                            warn!(url, attempt = attempt + 1, "Chrome returned empty DOM, retrying");
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
    pub(crate) async fn fetch(
        &self,
        url: &str,
        source_id: Uuid,
    ) -> Result<FetchedPage> {
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
