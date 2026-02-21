// Chrome + Browserless page fetchers.
// Moved from scout::pipeline::scraper in Phase 3.

use std::time::Duration;

use anyhow::{Context, Result};
use rand::Rng;
use rootsignal_common::ScrapedPage;
use tokio::sync::Semaphore;
use tracing::{info, warn};

use crate::readability::html_to_markdown;

/// Max concurrent Chromium processes. Each instance is heavy (~100MB+ RSS).
const MAX_CONCURRENT_CHROME: usize = 2;
/// Max retry attempts for transient Chrome failures.
const CHROME_MAX_ATTEMPTS: u32 = 3;
/// Base backoff duration for Chrome retries. Actual delay is base * 3^attempt + jitter.
const CHROME_RETRY_BASE: Duration = Duration::from_secs(3);

pub(crate) struct ChromeFetcher {
    semaphore: Semaphore,
}

impl ChromeFetcher {
    pub(crate) fn new() -> Self {
        info!("ChromeFetcher initialized (max_concurrent={MAX_CONCURRENT_CHROME})");
        Self {
            semaphore: Semaphore::new(MAX_CONCURRENT_CHROME),
        }
    }

    /// Fetch a page using headless Chrome, returning both raw HTML and markdown.
    pub(crate) async fn fetch(&self, url: &str) -> Result<ScrapedPage> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| anyhow::anyhow!("Chrome semaphore closed"))?;

        info!(url, fetcher = "chrome", "Fetching page");

        let html_bytes = self.run_chrome(url).await?;

        if html_bytes.is_empty() {
            warn!(url, fetcher = "chrome", "Empty DOM output");
            return Ok(ScrapedPage {
                url: url.to_string(),
                raw_html: String::new(),
                markdown: String::new(),
                content_hash: rootsignal_common::content_hash("").to_string(),
            });
        }

        let raw_html = String::from_utf8_lossy(&html_bytes).into_owned();
        let markdown = html_to_markdown(&html_bytes, Some(url));
        let hash = rootsignal_common::content_hash(&raw_html).to_string();

        info!(url, fetcher = "chrome", bytes = raw_html.len(), "Fetched successfully");

        Ok(ScrapedPage {
            url: url.to_string(),
            raw_html,
            markdown,
            content_hash: hash,
        })
    }

    /// Launch Chrome --dump-dom and return raw stdout bytes.
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
                    warn!(url, fetcher = "chrome", stderr = %stderr, "Chrome exited with error");
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

pub(crate) struct BrowserlessFetcher {
    client: browserless_client::BrowserlessClient,
}

impl BrowserlessFetcher {
    pub(crate) fn new(base_url: &str, token: Option<&str>) -> Self {
        info!(base_url, "BrowserlessFetcher initialized");
        Self {
            client: browserless_client::BrowserlessClient::new(base_url, token),
        }
    }

    /// Fetch a page via Browserless, returning both raw HTML and markdown.
    pub(crate) async fn fetch(&self, url: &str) -> Result<ScrapedPage> {
        info!(url, fetcher = "browserless", "Fetching page");

        let html = self
            .client
            .content(url)
            .await
            .context("Browserless content request failed")?;

        if html.is_empty() {
            warn!(url, fetcher = "browserless", "Empty HTML response");
            return Ok(ScrapedPage {
                url: url.to_string(),
                raw_html: String::new(),
                markdown: String::new(),
                content_hash: rootsignal_common::content_hash("").to_string(),
            });
        }

        let markdown = html_to_markdown(html.as_bytes(), Some(url));
        let hash = rootsignal_common::content_hash(&html).to_string();

        info!(url, fetcher = "browserless", bytes = html.len(), "Fetched successfully");

        Ok(ScrapedPage {
            url: url.to_string(),
            raw_html: html,
            markdown,
            content_hash: hash,
        })
    }
}

/// Extract links from raw HTML that match a given URL pattern.
/// Resolves relative URLs against `base_url`, deduplicates, and caps at 20 results.
pub(crate) fn extract_links_by_pattern(html: &str, base_url: &str, pattern: &str) -> Vec<String> {
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

fn is_transient_error(msg: &str) -> bool {
    msg.contains("Cannot fork") || msg.contains("Resource temporarily unavailable")
}

async fn retry_with_backoff(_url: &str, attempt: u32) {
    let backoff = CHROME_RETRY_BASE * 3u32.pow(attempt);
    let jitter = Duration::from_millis(rand::rng().random_range(0..1000));
    tokio::time::sleep(backoff + jitter).await;
}
